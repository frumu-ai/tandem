
#[tokio::test]
async fn coder_pr_review_reuses_prior_merge_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_merge_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-merge-memory-source",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 190
                }
            })
            .to_string(),
        ))
        .expect("create merge request");
    let create_merge_resp = app
        .clone()
        .oneshot(create_merge_req)
        .await
        .expect("create merge response");
    assert_eq!(create_merge_resp.status(), StatusCode::OK);

    let merge_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-merge-memory-source/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Hold merge until rollout validation completes.",
                "blockers": ["Rollout validation still pending"],
                "required_checks": ["staging-rollout"]
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

    let create_review_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-merge-memory-target",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 190
                }
            })
            .to_string(),
        ))
        .expect("create review request");
    let create_review_resp = app
        .clone()
        .oneshot(create_review_req)
        .await
        .expect("create review response");
    assert_eq!(create_review_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-merge-memory-target/memory-hits?q=rollout%20validation%20pending")
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
    let merge_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory")
            })
        })
        .cloned()
        .expect("merge recommendation hit");
    assert_eq!(
        merge_hit.get("same_ref").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        hits_payload
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
}

#[tokio::test]
async fn coder_merge_recommendation_reuses_prior_review_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_review_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-review-memory-source",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 191
                }
            })
            .to_string(),
        ))
        .expect("create review request");
    let create_review_resp = app
        .clone()
        .oneshot(create_review_req)
        .await
        .expect("create review response");
    assert_eq!(create_review_resp.status(), StatusCode::OK);

    let review_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-review-memory-source/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "Require rollout approval evidence before merge.",
                "requested_changes": ["Attach rollout approval evidence"],
                "blockers": ["Approval evidence missing"]
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

    let create_merge_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-review-memory-target",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 191
                }
            })
            .to_string(),
        ))
        .expect("create merge request");
    let create_merge_resp = app
        .clone()
        .oneshot(create_merge_req)
        .await
        .expect("create merge response");
    assert_eq!(create_merge_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-review-memory-target/memory-hits?q=approval%20evidence%20missing")
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
    let review_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("review_memory"))
        })
        .cloned()
        .expect("review memory hit");
    assert_eq!(
        review_hit.get("same_ref").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        hits_payload
            .get("retrieval_policy")
            .and_then(|row| row.get("prioritized_kinds"))
            .cloned(),
        Some(json!([
            "merge_recommendation_memory",
            "review_memory",
            "duplicate_linkage",
            "run_outcome",
            "regression_signal"
        ]))
    );
}

#[tokio::test]
async fn coder_promoted_review_memory_reuses_requested_changes_across_pull_requests() {
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
                "coder_run_id": "coder-review-promote-a",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 111
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-review-promote-a/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "Require rollback coverage before approval.",
                "risk_level": "high",
                "blockers": ["Rollback scenario coverage missing"],
                "requested_changes": ["Add rollback coverage"],
                "changed_files": ["crates/tandem-server/src/http/coder.rs"]
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
    let review_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("review_memory")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("review candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-review-promote-a/memory-candidates/{review_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable review guidance"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-review-promote-b",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 112
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-review-promote-b/memory-hits?q=rollback%20coverage")
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
    let promoted_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str)
                        == promote_payload.get("memory_id").and_then(Value::as_str)
            })
        })
        .cloned()
        .expect("promoted review hit");
    assert_eq!(
        promoted_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("review_memory")
    );
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("requested_changes: Add rollback coverage")));
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("blockers: Rollback scenario coverage missing")));
}

#[tokio::test]
async fn coder_promoted_regression_signal_reuses_across_pull_requests() {
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
                "coder_run_id": "coder-regression-promote-a",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 501
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-regression-promote-a/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "This change repeats the rollback-free migration pattern that regressed previously.",
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Rollback-free migrations regressed previously during deploy."
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
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let regression_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("regression_signal")).then(
                    || {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    },
                )?
            })
        })
        .expect("regression candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-regression-promote-a/memory-candidates/{regression_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable regression signal"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-regression-promote-b",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 502
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-regression-promote-b/memory-hits?q=rollback-free%20migrations%20regressed%20during%20deploy")
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
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("regression_signal")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
}

#[tokio::test]
async fn coder_merge_recommendation_reuses_promoted_regression_signal_across_pull_requests() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_review_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-regression-source",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 601
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("create review request");
    let create_review_resp = app
        .clone()
        .oneshot(create_review_req)
        .await
        .expect("create review response");
    assert_eq!(create_review_resp.status(), StatusCode::OK);

    let review_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-regression-source/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "This PR repeats the rollback-free deploy regression pattern and should not merge yet.",
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Rollback-free deploys regressed previously during release cutovers."
                }]
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
    let review_summary_payload: Value = serde_json::from_slice(
        &to_bytes(review_summary_resp.into_body(), usize::MAX)
            .await
            .expect("review summary body"),
    )
    .expect("review summary json");
    let regression_candidate_id = review_summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("regression_signal")).then(
                    || {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    },
                )?
            })
        })
        .expect("regression candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-merge-regression-source/memory-candidates/{regression_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable merge regression history"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_merge_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-regression-target",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 602
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("create merge request");
    let create_merge_resp = app
        .clone()
        .oneshot(create_merge_req)
        .await
        .expect("create merge response");
    assert_eq!(create_merge_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-regression-target/memory-hits")
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
            "user123/tandem pull request #602 merge recommendation regressions blockers required checks approvals"
        )
    );
    let promoted_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str)
                        == promote_payload.get("memory_id").and_then(Value::as_str)
            })
        })
        .cloned()
        .expect("promoted regression hit");
    assert_eq!(
        promoted_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("regression_signal")
    );
    assert_eq!(
        promoted_hit
            .get("metadata")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(promoted_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("Rollback-free deploys regressed previously")));
}

#[tokio::test]
async fn coder_promoted_fix_memory_reuses_strategy_across_issues() {
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
                "coder_run_id": "coder-fix-promote-a",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 201
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-fix-promote-a/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the startup fallback guard and cover the nil-config recovery path.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "validation_steps": ["cargo test -p tandem-server coder_promoted_fix_memory_reuses_strategy_across_issues -- --test-threads=1"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup fallback recovery regression passed"
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
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let fix_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("fix_pattern")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("fix candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-fix-promote-a/memory-candidates/{fix_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable fix pattern"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-fix-promote-b",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 202
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-fix-promote-b/memory-hits?q=startup%20fallback%20guard%20nil-config%20recovery")
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
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("fix_pattern")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(first_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("fix_strategy: add startup fallback guard")));
    assert!(first_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| {
            content.contains("root_cause: Startup recovery skipped the nil-config fallback path.")
        }));
}

#[tokio::test]
async fn coder_promoted_validation_memory_reuses_across_issues() {
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
                "coder_run_id": "coder-validation-promote-a",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 211
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-validation-promote-a/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the startup fallback guard and verify recovery with a targeted regression.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "validation_steps": ["cargo test -p tandem-server coder_promoted_validation_memory_reuses_across_issues -- --test-threads=1"],
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
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let validation_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("validation_memory")).then(
                    || {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    },
                )?
            })
        })
        .expect("validation candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-validation-promote-a/memory-candidates/{validation_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable validation evidence"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-validation-promote-b",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 212
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-validation-promote-b/memory-hits?q=startup%20recovery%20regression%20passed")
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
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("validation_memory")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(first_hit.get("content").and_then(Value::as_str).is_some());
}

#[tokio::test]
async fn coder_promoted_issue_fix_regression_signal_reuses_across_issues() {
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
                "coder_run_id": "coder-fix-regression-promote-a",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 213
                },
                "source_client": "desktop_developer_mode"
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

    let validation_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-fix-regression-promote-a/issue-fix-validation-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Guarded the startup recovery path, but the targeted regression still fails.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "guard fallback branch",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_steps": ["cargo test -p tandem-server coder_promoted_issue_fix_regression_signal_reuses_across_issues -- --test-threads=1"],
                "validation_results": [{
                    "kind": "test",
                    "status": "failed",
                    "summary": "startup recovery regression still fails"
                }]
            })
            .to_string(),
        ))
        .expect("validation request");
    let validation_resp = app
        .clone()
        .oneshot(validation_req)
        .await
        .expect("validation response");
    assert_eq!(validation_resp.status(), StatusCode::OK);
    let validation_payload: Value = serde_json::from_slice(
        &to_bytes(validation_resp.into_body(), usize::MAX)
            .await
            .expect("validation body"),
    )
    .expect("validation json");
    let regression_candidate_id = validation_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("regression_signal")).then(
                    || {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    },
                )?
            })
        })
        .expect("regression candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-fix-regression-promote-a/memory-candidates/{regression_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable issue-fix regression signal"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-fix-regression-promote-b",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 214
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-fix-regression-promote-b/memory-hits?q=startup%20recovery%20regression%20still%20fails")
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
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("regression_signal")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
}

#[tokio::test]
async fn coder_promoted_failure_pattern_reuses_across_issues() {
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
                "coder_run_id": "coder-failure-pattern-promote-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 301
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-failure-pattern-promote-a/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "GitHub capability bindings drifted, so issue triage failed before reproduction.",
                "confidence": "high",
                "likely_root_cause": "Capability readiness drift in GitHub issue bindings.",
                "reproduction": "Run creation halted before reproduction because GitHub issue capabilities were missing."
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
    let failure_pattern_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("failure_pattern")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("failure pattern candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-failure-pattern-promote-a/memory-candidates/{failure_pattern_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable failure pattern"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-failure-pattern-promote-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 302
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-failure-pattern-promote-b/memory-hits?q=github%20capability%20bindings%20drift%20issue%20triage%20reproduction%20missing")
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
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("failure_pattern")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(first_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("GitHub capability bindings drifted")));
    assert!(first_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("issue triage failed before reproduction")));
}

#[tokio::test]
async fn coder_promoted_triage_outcome_reuses_across_issues() {
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
                "coder_run_id": "coder-triage-outcome-promote-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 401
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-triage-outcome-promote-a/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Capability readiness drift in GitHub issue bindings is the likely root cause.",
                "confidence": "high",
                "likely_root_cause": "GitHub issue bindings were not connected when triage started."
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
    let run_outcome_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("run_outcome")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("run outcome candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-triage-outcome-promote-a/memory-candidates/{run_outcome_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable triage outcome"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-outcome-promote-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 402
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-triage-outcome-promote-b/memory-hits?q=issue%20triage%20completed%20high%20confidence")
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
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("run_outcome")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(first_hit.get("content").and_then(Value::as_str).is_some());
}

#[tokio::test]
async fn coder_promoted_triage_regression_signal_reuses_across_issues() {
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
                "coder_run_id": "coder-triage-regression-promote-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 451
                },
                "source_client": "desktop_developer_mode"
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

    let repro_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-regression-promote-a/triage-reproduction-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "outcome": "failed_to_reproduce",
                "steps": [
                    "Start the triage workflow against a misconfigured GitHub runtime",
                    "Observe the same readiness failure before reproduction can complete"
                ],
                "observed_logs": [
                    "GitHub capability bindings drifted from the expected project setup"
                ],
                "memory_hits_used": ["memory-hit-triage-regression-1"],
                "notes": "Keep this regression signal for future issue triage."
            })
            .to_string(),
        ))
        .expect("repro request");
    let repro_resp = app
        .clone()
        .oneshot(repro_req)
        .await
        .expect("repro response");
    assert_eq!(repro_resp.status(), StatusCode::OK);
    let repro_payload: Value = serde_json::from_slice(
        &to_bytes(repro_resp.into_body(), usize::MAX)
            .await
            .expect("repro body"),
    )
    .expect("repro json");
    let regression_signal_candidate_id = repro_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("regression_signal")).then(
                    || {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    },
                )?
            })
        })
        .expect("regression signal candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-triage-regression-promote-a/memory-candidates/{regression_signal_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable triage regression signal"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-regression-promote-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 452
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-triage-regression-promote-b/memory-hits?q=Issue%20triage%20regression%20signal%20failed_to_reproduce%20Keep%20this%20regression%20signal")
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
    let hits = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .cloned()
        .expect("hits");
    let first_hit = hits.first().cloned().expect("first hit");
    assert_eq!(
        first_hit
            .get("kind")
            .or_else(|| first_hit.get("metadata").and_then(|row| row.get("kind")))
            .and_then(Value::as_str),
        Some("regression_signal")
    );
    let governed_hit = hits
        .iter()
        .find(|row| {
            row.get("memory_id").and_then(Value::as_str)
                == promote_payload.get("memory_id").and_then(Value::as_str)
        })
        .cloned()
        .expect("governed regression signal hit");
    assert_eq!(
        governed_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(governed_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(governed_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("Keep this regression signal")));
}
