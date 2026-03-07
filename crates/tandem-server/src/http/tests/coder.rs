use super::*;
use tandem_memory::types::GlobalMemoryRecord;

#[tokio::test]
async fn coder_issue_triage_run_create_get_and_list() {
    let state = test_state().await;
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
                "coder_run_id": "coder-run-1",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 1234,
                    "url": "https://github.com/evan/tandem/issues/1234"
                },
                "source_client": "desktop_developer_mode"
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
    assert_eq!(
        create_payload
            .get("coder_run")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        create_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("bootstrapping")
    );
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_issue_triage")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("planning")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(5)
    );
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
        Some("evan/tandem issue #1234")
    );
    assert_eq!(
        get_payload
            .get("memory_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );

    let list_req = Request::builder()
        .method("GET")
        .uri("/coder/runs?workflow_mode=issue_triage")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    assert_eq!(
        list_payload
            .get("runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        list_payload
            .get("runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("linked_context_run_id"))
            .and_then(Value::as_str),
        Some(linked_context_run_id.as_str())
    );

    let artifact_event = next_event_of_type(&mut rx, "coder.artifact.added").await;
    assert_eq!(
        artifact_event
            .properties
            .get("workflow_mode")
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        artifact_event
            .properties
            .get("kind")
            .and_then(Value::as_str),
        Some("memory_hits")
    );
    assert_eq!(
        artifact_event
            .properties
            .get("phase")
            .and_then(Value::as_str),
        Some("memory_retrieval")
    );

    let created_event = next_event_of_type(&mut rx, "coder.run.created").await;
    assert_eq!(
        created_event
            .properties
            .get("workflow_mode")
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        created_event
            .properties
            .get("repo_binding")
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("evan/tandem")
    );
    assert_eq!(
        created_event
            .properties
            .get("phase")
            .and_then(Value::as_str),
        Some("bootstrapping")
    );
}

#[tokio::test]
async fn coder_pr_review_run_create_gets_seeded_review_tasks() {
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
                "coder_run_id": "coder-pr-review-1",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 88,
                    "url": "https://github.com/evan/tandem/pull/88"
                },
                "source_client": "desktop_developer_mode"
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
    assert_eq!(
        create_payload
            .get("coder_run")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_pr_review")
    );
    assert!(get_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
        }))
        .unwrap_or(false));
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len())
            .filter(|count| *count >= 3),
        Some(
            get_payload
                .get("run")
                .and_then(|row| row.get("tasks"))
                .and_then(Value::as_array)
                .map(|rows| rows.len())
                .unwrap_or_default()
        )
    );
    assert!(get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("workflow_node_id").and_then(Value::as_str) == Some("inspect_pull_request")
        }))
        .unwrap_or(false));
    assert!(get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("workflow_node_id").and_then(Value::as_str) == Some("review_pull_request")
        }))
        .unwrap_or(false));
    assert_eq!(
        get_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("bootstrapping")
    );
    assert_eq!(
        get_payload
            .get("coder_run")
            .and_then(|row| row.get("linked_context_run_id"))
            .and_then(Value::as_str),
        Some(linked_context_run_id.as_str())
    );

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-1/memory-hits")
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
        Some("evan/tandem pull request #88")
    );
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
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 89,
                    "url": "https://github.com/evan/tandem/pull/89"
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
    let summary_artifact_id = summary_payload
        .get("artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("summary artifact id")
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

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.run_type, "coder_pr_review");
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
                    "repo_slug": "evan/tandem"
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
                    "repo_slug": "evan/tandem"
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
        Some("evan/tandem pull request #90")
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
                    "repo_slug": "evan/tandem"
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
        Some("evan/tandem pull request #91")
    );
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
                    "repo_slug": "evan/tandem"
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
                    "repo_slug": "evan/tandem"
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
        Some("bootstrapping")
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

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-replay",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 404,
                    "url": "https://github.com/evan/tandem/issues/404"
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
        .expect("linked context run id")
        .to_string();

    let checkpoint_req = Request::builder()
        .method("POST")
        .uri(format!("/context/runs/{linked_context_run_id}/checkpoints"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "coder_replay_regression"
            })
            .to_string(),
        ))
        .expect("checkpoint request");
    let checkpoint_resp = app
        .clone()
        .oneshot(checkpoint_req)
        .await
        .expect("checkpoint response");
    assert_eq!(checkpoint_resp.status(), StatusCode::OK);
    let checkpoint_body = to_bytes(checkpoint_resp.into_body(), usize::MAX)
        .await
        .expect("checkpoint body");
    let checkpoint_payload: Value =
        serde_json::from_slice(&checkpoint_body).expect("checkpoint json");
    assert_eq!(
        checkpoint_payload
            .get("checkpoint")
            .and_then(|row| row.get("run_id"))
            .and_then(Value::as_str),
        Some(linked_context_run_id.as_str())
    );

    let replay_req = Request::builder()
        .method("GET")
        .uri(format!("/context/runs/{linked_context_run_id}/replay"))
        .body(Body::empty())
        .expect("replay request");
    let replay_resp = app
        .clone()
        .oneshot(replay_req)
        .await
        .expect("replay response");
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
        .await
        .expect("replay body");
    let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay json");

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

#[tokio::test]
async fn coder_artifacts_endpoint_projects_context_blackboard_artifacts() {
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
                "coder_run_id": "coder-run-2",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 9
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

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-2/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_body = to_bytes(artifacts_resp.into_body(), usize::MAX)
        .await
        .expect("artifacts body");
    let artifacts_payload: Value = serde_json::from_slice(&artifacts_body).expect("artifacts json");
    let contains_memory_hits = artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
            })
        })
        .unwrap_or(false);
    assert!(contains_memory_hits);
}

#[tokio::test]
async fn coder_issue_triage_blocks_when_preferred_mcp_server_is_missing() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state);

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 42
                },
                "mcp_servers": ["missing-github"]
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::CONFLICT);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("CODER_READINESS_BLOCKED")
    );
}

#[tokio::test]
async fn coder_memory_candidate_create_persists_artifact() {
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
                "coder_run_id": "coder-run-3",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 77
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

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-3/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "triage_memory",
                "summary": "Likely duplicate failure",
                "payload": {
                    "confidence": "medium"
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

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-3/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_body = to_bytes(artifacts_resp.into_body(), usize::MAX)
        .await
        .expect("artifacts body");
    let artifacts_payload: Value = serde_json::from_slice(&artifacts_body).expect("artifacts json");
    let contains_candidate = artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_candidate")
            })
        })
        .unwrap_or(false);
    assert!(contains_candidate);
}

#[tokio::test]
async fn coder_issue_triage_seeds_ranked_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let first_run_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-seed-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 88
                }
            })
            .to_string(),
        ))
        .expect("first run request");
    let first_run_resp = app
        .clone()
        .oneshot(first_run_req)
        .await
        .expect("first run response");
    assert_eq!(first_run_resp.status(), StatusCode::OK);

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-seed-a/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "Known duplicate failure",
                "payload": {
                    "label": "duplicate"
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

    let second_run_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-seed-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 88
                }
            })
            .to_string(),
        ))
        .expect("second run request");
    let second_run_resp = app
        .clone()
        .oneshot(second_run_req)
        .await
        .expect("second run response");
    assert_eq!(second_run_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-seed-b")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    let retrieve_task = get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .and_then(|tasks| {
            tasks.iter().find(|task| {
                task.get("workflow_node_id").and_then(Value::as_str) == Some("retrieve_memory")
            })
        })
        .cloned()
        .expect("retrieve task");
    let hint_count = retrieve_task
        .get("payload")
        .and_then(|row| row.get("memory_hits"))
        .and_then(Value::as_array)
        .map(|rows| rows.len())
        .unwrap_or(0);
    assert!(hint_count >= 1);
}

#[tokio::test]
async fn coder_memory_hits_endpoint_returns_ranked_hits() {
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
                "coder_run_id": "coder-run-hits-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 95
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

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-hits-a/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "triage_memory",
                "summary": "Repeated issue near capability readiness",
                "payload": {
                    "tag": "known"
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

    let second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-hits-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 95
                }
            })
            .to_string(),
        ))
        .expect("second request");
    let second_resp = app
        .clone()
        .oneshot(second_req)
        .await
        .expect("second response");
    assert_eq!(second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-hits-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_body = to_bytes(hits_resp.into_body(), usize::MAX)
        .await
        .expect("hits body");
    let hits_payload: Value = serde_json::from_slice(&hits_body).expect("hits json");
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| !rows.is_empty())
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_issue_triage_retrieves_governed_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let db = super::super::skills_memory::open_global_memory_db()
        .await
        .expect("global memory db");
    db.put_global_memory_record(&GlobalMemoryRecord {
        id: "memory-governed-1".to_string(),
        user_id: "desktop_developer_mode".to_string(),
        source_type: "solution_capsule".to_string(),
        content: "Past triage found capability readiness drift in coder issue triage setup"
            .to_string(),
        content_hash: String::new(),
        run_id: "memory-run-1".to_string(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some("proj-engine".to_string()),
        channel_tag: None,
        host_tag: None,
        metadata: Some(json!({
            "kind": "triage_memory"
        })),
        provenance: Some(json!({
            "origin_event_type": "memory.put"
        })),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        expires_at_ms: None,
    })
    .await
    .expect("seed governed memory");

    let app = app_router(state.clone());
    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-governed-hits",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 202
                },
                "source_client": "desktop_developer_mode"
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

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-governed-hits/memory-hits?q=capability%20readiness")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_body = to_bytes(hits_resp.into_body(), usize::MAX)
        .await
        .expect("hits body");
    let hits_payload: Value = serde_json::from_slice(&hits_body).expect("hits json");
    let has_governed_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str) == Some("memory-governed-1")
            })
        })
        .unwrap_or(false);
    assert!(has_governed_hit);
}

#[tokio::test]
async fn coder_triage_summary_write_adds_summary_artifact() {
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
                "coder_run_id": "coder-run-summary",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-summary/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Likely duplicate in capabilities flow",
                "confidence": "medium",
                "affected_files": ["crates/tandem-server/src/http/coder.rs"],
                "memory_hits_used": ["memcand-1"]
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
    let summary_body = to_bytes(summary_resp.into_body(), usize::MAX)
        .await
        .expect("summary body");
    let summary_payload: Value = serde_json::from_slice(&summary_body).expect("summary json");
    let generated_candidates = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(generated_candidates
        .iter()
        .any(|row| { row.get("kind").and_then(Value::as_str) == Some("triage_memory") }));
    assert!(generated_candidates
        .iter()
        .any(|row| { row.get("kind").and_then(Value::as_str) == Some("failure_pattern") }));
    assert!(generated_candidates
        .iter()
        .any(|row| { row.get("kind").and_then(Value::as_str) == Some("run_outcome") }));

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-summary/memory-candidates")
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
    let failure_pattern_payload = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("failure_pattern"))
        })
        .and_then(|row| row.get("payload"))
        .cloned()
        .expect("failure pattern payload");
    assert_eq!(
        failure_pattern_payload.get("type").and_then(Value::as_str),
        Some("failure.pattern")
    );
    assert_eq!(
        failure_pattern_payload
            .get("linked_issue_numbers")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_u64),
        Some(91)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-summary/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_body = to_bytes(artifacts_resp.into_body(), usize::MAX)
        .await
        .expect("artifacts body");
    let artifacts_payload: Value = serde_json::from_slice(&artifacts_body).expect("artifacts json");
    let contains_summary = artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_triage_summary")
            })
        })
        .unwrap_or(false);
    assert!(contains_summary);

    let contains_memory_hits = artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
            })
        })
        .unwrap_or(false);
    assert!(contains_memory_hits);

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-summary/memory-candidates")
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
    let kinds = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("kind").and_then(Value::as_str))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(kinds.contains(&"triage_memory"));
    assert!(kinds.contains(&"run_outcome"));
}

#[tokio::test]
async fn coder_memory_candidate_promote_stores_governed_memory() {
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
                "coder_run_id": "coder-run-promote",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 333
                },
                "source_client": "desktop_developer_mode"
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
        .uri("/coder/runs/coder-run-promote/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Capability readiness drift already explained this failure",
                "confidence": "high"
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
    let summary_body = to_bytes(summary_resp.into_body(), usize::MAX)
        .await
        .expect("summary body");
    let summary_payload: Value = serde_json::from_slice(&summary_body).expect("summary json");
    let triage_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("triage_memory")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("triage candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-run-promote/memory-candidates/{triage_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable triage memory"
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
    let promote_body = to_bytes(promote_resp.into_body(), usize::MAX)
        .await
        .expect("promote body");
    let promote_payload: Value = serde_json::from_slice(&promote_body).expect("promote json");
    assert_eq!(
        promote_payload.get("promoted").and_then(Value::as_bool),
        Some(true)
    );

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-promote/memory-hits?q=capability%20readiness")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_body = to_bytes(hits_resp.into_body(), usize::MAX)
        .await
        .expect("hits body");
    let hits_payload: Value = serde_json::from_slice(&hits_body).expect("hits json");
    let has_promoted_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str)
                        == promote_payload.get("memory_id").and_then(Value::as_str)
            })
        })
        .unwrap_or(false);
    assert!(has_promoted_hit);
}
