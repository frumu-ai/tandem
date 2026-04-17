
#[tokio::test]
async fn coder_issue_fix_run_replay_matches_persisted_state_and_checkpoint() {
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
            "coder_run_id": "coder-run-fix-replay",
            "workflow_mode": "issue_fix",
            "repo_binding": {
                "project_id": "proj-engine",
                "workspace_id": "ws-tandem",
                "workspace_root": "/tmp/tandem-repo",
                "repo_slug": "user123/tandem",
                "default_branch": "main"
            },
            "github_ref": {
                "kind": "issue",
                "number": 405,
                "url": "https://github.com/user123/tandem/issues/405"
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
        Some("coder_issue_fix")
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
            .map(|rows| rows.iter().any(|row| {
                row.get("workflow_node_id").and_then(Value::as_str) == Some("implement_patch")
            })),
        Some(true)
    );
}

#[tokio::test]
async fn coder_pr_review_run_replay_matches_persisted_state_and_checkpoint() {
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
            "coder_run_id": "coder-run-review-replay",
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
                "number": 406,
                "url": "https://github.com/user123/tandem/pull/406"
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
        Some("coder_pr_review")
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
            .map(|rows| rows.iter().any(|row| {
                row.get("workflow_node_id").and_then(Value::as_str) == Some("review_pull_request")
            })),
        Some(true)
    );
}

#[tokio::test]
async fn coder_merge_recommendation_run_replay_matches_persisted_state_and_checkpoint() {
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
            "coder_run_id": "coder-run-merge-replay",
            "workflow_mode": "merge_recommendation",
            "repo_binding": {
                "project_id": "proj-engine",
                "workspace_id": "ws-tandem",
                "workspace_root": "/tmp/tandem-repo",
                "repo_slug": "user123/tandem",
                "default_branch": "main"
            },
            "github_ref": {
                "kind": "pull_request",
                "number": 407,
                "url": "https://github.com/user123/tandem/pull/407"
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
        Some("coder_merge_recommendation")
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
            .map(|rows| rows.iter().any(|row| {
                row.get("workflow_node_id").and_then(Value::as_str)
                    == Some("assess_merge_readiness")
            })),
        Some(true)
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
                    "repo_slug": "user123/tandem"
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
                    "repo_slug": "user123/tandem"
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
                    "repo_slug": "user123/tandem"
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
                    "repo_slug": "user123/tandem"
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
                    "repo_slug": "user123/tandem"
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
    let duplicate_count = retrieve_task
        .get("payload")
        .and_then(|row| row.get("duplicate_candidates"))
        .and_then(Value::as_array)
        .map(|rows| rows.len())
        .unwrap_or(0);
    assert!(duplicate_count >= 1);
}

#[tokio::test]
async fn coder_triage_reproduction_report_advances_triage_run() {
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
                "coder_run_id": "coder-triage-repro",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 96
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

    let repro_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-repro/triage-reproduction-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Reproduced the capability-readiness issue when bindings are missing.",
                "outcome": "reproduced",
                "steps": [
                    "Disconnect GitHub MCP bindings",
                    "Create issue_triage coder run"
                ],
                "observed_logs": [
                    "capabilities readiness failed closed"
                ],
                "affected_files": ["crates/tandem-server/src/http/coder.rs"],
                "memory_hits_used": ["memory-hit-triage-repro-1"]
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
    assert_eq!(
        repro_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_reproduction_report")
    );
    assert_eq!(
        repro_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        repro_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("artifact_write")
    );

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    for workflow_node_id in ["inspect_repo", "attempt_reproduction"] {
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
            .find(|task| task.workflow_node_id.as_deref() == Some("write_triage_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_triage_reproduction_failed_writes_run_outcome_candidate() {
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
                "coder_run_id": "coder-triage-repro-failed",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 196
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

    let repro_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-repro-failed/triage-reproduction-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "outcome": "failed_to_reproduce",
                "steps": [
                    "Run issue triage with missing runtime condition",
                    "Observe no deterministic reproduction"
                ],
                "observed_logs": [
                    "capability readiness blocked execution"
                ],
                "memory_hits_used": ["memory-hit-triage-failure-1"],
                "notes": "Preserve this failure outcome for future triage ranking."
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
    assert_eq!(
        repro_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| row.get("kind").and_then(Value::as_str) == Some("run_outcome"))),
        Some(true)
    );
    assert_eq!(
        repro_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| row.get("kind").and_then(Value::as_str) == Some("regression_signal"))),
        Some(true)
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-triage-repro-failed/memory-candidates")
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
    let run_outcome_payload = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("run_outcome"))
        })
        .and_then(|row| row.get("payload"))
        .cloned()
        .expect("run outcome payload");
    assert_eq!(
        run_outcome_payload.get("result").and_then(Value::as_str),
        Some("triage_reproduction_failed")
    );
    assert_eq!(
        run_outcome_payload
            .get("reproduction")
            .and_then(|row| row.get("outcome"))
            .and_then(Value::as_str),
        Some("failed_to_reproduce")
    );
    let regression_signal_payload = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("regression_signal"))
        })
        .and_then(|row| row.get("payload"))
        .cloned()
        .expect("regression signal payload");
    assert_eq!(
        regression_signal_payload
            .get("result")
            .and_then(Value::as_str),
        Some("triage_reproduction_failed")
    );
    assert_eq!(
        regression_signal_payload
            .get("regression_signals")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("triage_reproduction_failed")
    );
}

#[tokio::test]
async fn coder_triage_reproduction_report_infers_memory_and_prior_runs() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_seed_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-repro-seed",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 296
                }
            })
            .to_string(),
        ))
        .expect("seed create request");
    let create_seed_resp = app
        .clone()
        .oneshot(create_seed_req)
        .await
        .expect("seed create response");
    assert_eq!(create_seed_resp.status(), StatusCode::OK);

    let seed_candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-repro-seed/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "task_id": "attempt_reproduction",
                "summary": "Prior startup recovery failure signature for reproduction context.",
                "payload": {
                    "workflow_mode": "issue_triage",
                    "summary": "Prior startup recovery failure signature for reproduction context.",
                    "fingerprint": "triage-repro-seed-fingerprint",
                    "canonical_markers": ["startup recovery", "repro signal"],
                    "linked_issue_numbers": [296]
                }
            })
            .to_string(),
        ))
        .expect("seed candidate request");
    let seed_candidate_resp = app
        .clone()
        .oneshot(seed_candidate_req)
        .await
        .expect("seed candidate response");
    assert_eq!(seed_candidate_resp.status(), StatusCode::OK);
    let seed_candidate_payload: Value = serde_json::from_slice(
        &to_bytes(seed_candidate_resp.into_body(), usize::MAX)
            .await
            .expect("seed candidate body"),
    )
    .expect("seed candidate json");
    let seeded_candidate_id = seed_candidate_payload
        .get("candidate_id")
        .and_then(Value::as_str)
        .expect("seeded candidate id")
        .to_string();

    let create_target_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-repro-inferred",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 296
                }
            })
            .to_string(),
        ))
        .expect("target create request");
    let create_target_resp = app
        .clone()
        .oneshot(create_target_req)
        .await
        .expect("target create response");
    assert_eq!(create_target_resp.status(), StatusCode::OK);

    let repro_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-repro-inferred/triage-reproduction-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Reproduction report without explicit memory hit ids.",
                "outcome": "reproduced",
                "steps": [
                    "Run triage execution path",
                    "Inspect previous failure markers first"
                ],
                "observed_logs": [
                    "reused prior startup recovery signal"
                ]
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
    let repro_artifact_path = repro_payload
        .get("artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .expect("repro artifact path");
    let repro_artifact_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(repro_artifact_path)
            .await
            .expect("read repro artifact"),
    )
    .expect("parse repro artifact");
    assert_eq!(
        repro_artifact_payload
            .get("memory_hits_used")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| row.as_str() == Some(seeded_candidate_id.as_str()))),
        Some(true)
    );
    assert_eq!(
        repro_artifact_payload
            .get("prior_runs_considered")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter().any(|row| {
                    row.get("coder_run_id").and_then(Value::as_str)
                        == Some("coder-triage-repro-seed")
                })
            }),
        Some(true)
    );
}

#[tokio::test]
async fn coder_triage_inspection_report_advances_to_reproduction() {
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
                "coder_run_id": "coder-triage-inspection",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 97
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

    let inspection_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-inspection/triage-inspection-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "The repo inspection points at capability readiness and MCP binding setup.",
                "likely_areas": ["capability resolver", "github readiness"],
                "affected_files": ["crates/tandem-server/src/http/coder.rs"],
                "memory_hits_used": ["memory-hit-triage-inspection-1"],
                "notes": "Inspection completed before reproduction."
            })
            .to_string(),
        ))
        .expect("inspection request");
    let inspection_resp = app
        .clone()
        .oneshot(inspection_req)
        .await
        .expect("inspection response");
    assert_eq!(inspection_resp.status(), StatusCode::OK);
    let inspection_payload: Value = serde_json::from_slice(
        &to_bytes(inspection_resp.into_body(), usize::MAX)
            .await
            .expect("inspection body"),
    )
    .expect("inspection json");
    assert_eq!(
        inspection_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_repo_inspection_report")
    );
    assert_eq!(
        inspection_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        inspection_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("reproduction")
    );

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("inspect_repo"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Done)
    );
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("attempt_reproduction"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_triage_summary_infers_duplicate_linkage_from_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let seed_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-duplicate-linkage-seed",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 512
                }
            })
            .to_string(),
        ))
        .expect("seed request");
    let seed_resp = app.clone().oneshot(seed_req).await.expect("seed response");
    assert_eq!(seed_resp.status(), StatusCode::OK);

    let seed_candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-duplicate-linkage-seed/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "duplicate_linkage",
                "task_id": "retrieve_memory",
                "summary": "user123/tandem issue #512 is already linked to pull request #913",
                "payload": {
                    "type": "duplicate.issue_pr_linkage",
                    "repo_slug": "user123/tandem",
                    "project_id": "proj-engine",
                    "summary": "user123/tandem issue #512 is already linked to pull request #913",
                    "linked_issue_numbers": [512],
                    "linked_pr_numbers": [913],
                    "relationship": "historical_duplicate_linkage",
                    "artifact_refs": ["artifacts/pr_submission.json"]
                }
            })
            .to_string(),
        ))
        .expect("seed candidate request");
    let seed_candidate_resp = app
        .clone()
        .oneshot(seed_candidate_req)
        .await
        .expect("seed candidate response");
    assert_eq!(seed_candidate_resp.status(), StatusCode::OK);

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-duplicate-linkage-target",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 512
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
        .uri("/coder/runs/coder-triage-duplicate-linkage-target/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "This issue is likely already covered by an existing pull request.",
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
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| row.get("kind").and_then(Value::as_str) == Some("duplicate_linkage"))),
        Some(true)
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-triage-duplicate-linkage-target/memory-candidates")
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
        .cloned()
        .expect("triage duplicate linkage");
    assert_eq!(
        duplicate_linkage
            .get("payload")
            .and_then(|row| row.get("linked_issue_numbers"))
            .cloned(),
        Some(json!([512]))
    );
    assert_eq!(
        duplicate_linkage
            .get("payload")
            .and_then(|row| row.get("linked_pr_numbers"))
            .cloned(),
        Some(json!([913]))
    );
    assert_eq!(
        duplicate_linkage
            .get("payload")
            .and_then(|row| row.get("relationship"))
            .and_then(Value::as_str),
        Some("issue_triage_duplicate_inference")
    );
}

#[tokio::test]
async fn coder_issue_triage_execute_next_drives_task_runtime_to_completion() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let seed_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-execute-seed",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 197
                }
            })
            .to_string(),
        ))
        .expect("seed request");
    let seed_resp = app.clone().oneshot(seed_req).await.expect("seed response");
    assert_eq!(seed_resp.status(), StatusCode::OK);

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-execute-seed/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "Known startup recovery duplicate",
                "payload": {
                    "type": "failure.pattern",
                    "repo_slug": "user123/tandem",
                    "fingerprint": "triage-execute-duplicate",
                    "symptoms": ["startup recovery", "issue triage"],
                    "canonical_markers": ["startup recovery", "issue triage", "user123/tandem issue #198"],
                    "linked_issue_numbers": [198],
                    "recurrence_count": 2,
                    "linked_pr_numbers": [],
                    "affected_components": ["coder"],
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
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-execute-next",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 198
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
        "inspect_repo",
        "attempt_reproduction",
        "write_triage_artifact",
    ] {
        let execute_req = Request::builder()
            .method("POST")
            .uri("/coder/runs/coder-triage-execute-next/execute-next")
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
    let triage_summary_path = blackboard
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_type == "coder_triage_summary")
        .map(|artifact| artifact.path.clone())
        .expect("triage summary path");
    let triage_summary_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(&triage_summary_path)
            .await
            .expect("read triage summary"),
    )
    .expect("triage summary json");
    assert!(triage_summary_payload
        .get("duplicate_candidates")
        .and_then(Value::as_array)
        .map(|rows| !rows.is_empty())
        .unwrap_or(false));
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("inspect_repo"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Done)
    );
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("attempt_reproduction"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Done)
    );
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("write_triage_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Done)
    );
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
                    "repo_slug": "user123/tandem"
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

    let failure_pattern_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-hits-a/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "Capability readiness drift repeatedly blocks issue triage startup.",
                "payload": {
                    "type": "historical_failure_pattern",
                    "root_cause": "GitHub capability bindings were missing during run bootstrap."
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
                    "repo_slug": "user123/tandem"
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
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("failure_pattern")
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
    assert_eq!(
        hits_payload
            .get("retrieval_policy")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        hits_payload
            .get("retrieval_policy")
            .and_then(|row| row.get("sources"))
            .cloned(),
        Some(json!([
            "repo_memory_candidates",
            "project_memory",
            "governed_memory"
        ]))
    );
    assert_eq!(
        hits_payload
            .get("retrieval_policy")
            .and_then(|row| row.get("prioritized_kinds"))
            .cloned(),
        Some(json!([
            "failure_pattern",
            "regression_signal",
            "duplicate_linkage",
            "triage_memory",
            "fix_pattern",
            "run_outcome"
        ]))
    );
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
