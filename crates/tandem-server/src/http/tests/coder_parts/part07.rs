
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
                    "repo_slug": "user123/tandem"
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
        .uri("/coder/runs/coder-run-summary/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Likely duplicate in capabilities flow",
                "confidence": "medium",
                "affected_files": ["crates/tandem-server/src/http/coder.rs"],
                "prior_runs_considered": [{
                    "coder_run_id": "coder-run-prior-a",
                    "linked_context_run_id": "ctx-coder-run-prior-a",
                    "kind": "failure_pattern",
                    "tier": "project"
                }],
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
    let triage_memory_payload = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("triage_memory"))
        })
        .and_then(|row| row.get("payload"))
        .cloned()
        .expect("triage memory payload");
    assert_eq!(
        triage_memory_payload
            .get("prior_runs_considered")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-run-prior-a")
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(3)
    );
    assert_eq!(
        summary_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
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
    let summary_path = load_context_blackboard(&state, &linked_context_run_id)
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_type == "coder_triage_summary")
        .map(|artifact| artifact.path.clone())
        .expect("triage summary artifact path");
    let summary_artifact_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(&summary_path)
            .await
            .expect("read triage summary artifact"),
    )
    .expect("triage summary artifact json");
    assert_eq!(
        summary_artifact_payload
            .get("prior_runs_considered")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-run-prior-a")
    );

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
    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_triage_summary"));
    for workflow_node_id in [
        "ingest_reference",
        "retrieve_memory",
        "inspect_repo",
        "attempt_reproduction",
        "write_triage_artifact",
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
async fn coder_triage_summary_writes_run_outcome_without_summary_text() {
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
                "coder_run_id": "coder-run-triage-outcome-only",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 191
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
        .uri("/coder/runs/coder-run-triage-outcome-only/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "confidence": "low",
                "reproduction": {
                    "outcome": "failed_to_reproduce",
                    "steps": ["cargo test -p tandem-server missing_case -- --test-threads=1"]
                },
                "notes": "Issue triage failed before reliable reproduction but should still keep an outcome."
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
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("run_outcome") })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("triage_memory") })),
        Some(false)
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-triage-outcome-only/memory-candidates")
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
        run_outcome_payload.get("summary").and_then(Value::as_str),
        Some("Issue triage reproduction outcome: failed_to_reproduce")
    );
}

#[tokio::test]
async fn coder_triage_summary_infers_duplicate_and_memory_fields_from_bootstrap_hits() {
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
                "coder_run_id": "coder-run-triage-seed",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 601
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
        .uri("/coder/runs/coder-run-triage-seed/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "task_id": "attempt_reproduction",
                "summary": "Seeded startup recovery failure pattern",
                "payload": {
                    "workflow_mode": "issue_triage",
                    "summary": "Seeded startup recovery failure pattern",
                    "fingerprint": "seeded-startup-recovery-fingerprint",
                    "canonical_markers": ["startup recovery", "panic"],
                    "linked_issue_numbers": [601],
                    "affected_components": ["crates/tandem-server/src/http/coder.rs"]
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
                "coder_run_id": "coder-run-triage-auto-fields",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 601
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
    let create_target_payload: Value = serde_json::from_slice(
        &to_bytes(create_target_resp.into_body(), usize::MAX)
            .await
            .expect("target create body"),
    )
    .expect("target create json");
    let target_context_run_id = create_target_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("target linked context run id")
        .to_string();

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-triage-auto-fields/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Automatically infer duplicate and memory provenance fields.",
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

    let triage_summary_path = load_context_blackboard(&state, &target_context_run_id)
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_type == "coder_triage_summary")
        .map(|artifact| artifact.path.clone())
        .expect("triage summary artifact path");
    let triage_summary_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(&triage_summary_path)
            .await
            .expect("read triage summary artifact"),
    )
    .expect("parse triage summary artifact");
    assert_eq!(
        triage_summary_payload
            .get("memory_hits_used")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| row.as_str() == Some(seeded_candidate_id.as_str()))),
        Some(true)
    );
    assert_eq!(
        triage_summary_payload
            .get("prior_runs_considered")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter().any(|row| {
                    row.get("coder_run_id").and_then(Value::as_str) == Some("coder-run-triage-seed")
                })
            }),
        Some(true)
    );
    assert_eq!(
        triage_summary_payload
            .get("duplicate_candidates")
            .and_then(Value::as_array)
            .map(|rows| !rows.is_empty()),
        Some(true)
    );
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
                    "repo_slug": "user123/tandem"
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

    let create_fix_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-promote-fix",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 334
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("create fix request");
    let create_fix_resp = app
        .clone()
        .oneshot(create_fix_req)
        .await
        .expect("create fix response");
    assert_eq!(create_fix_resp.status(), StatusCode::OK);

    let fix_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-promote-fix/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the missing startup fallback guard and validate recovery behavior.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"]
            })
            .to_string(),
        ))
        .expect("fix summary request");
    let fix_summary_resp = app
        .clone()
        .oneshot(fix_summary_req)
        .await
        .expect("fix summary response");
    assert_eq!(fix_summary_resp.status(), StatusCode::OK);
    let fix_summary_payload: Value = serde_json::from_slice(
        &to_bytes(fix_summary_resp.into_body(), usize::MAX)
            .await
            .expect("fix summary body"),
    )
    .expect("fix summary json");
    let fix_pattern_candidate_id = fix_summary_payload
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
        .expect("fix pattern candidate id");

    let promote_fix_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-run-promote-fix/memory-candidates/{fix_pattern_candidate_id}/promote"
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
        .expect("promote fix request");
    let promote_fix_resp = app
        .clone()
        .oneshot(promote_fix_req)
        .await
        .expect("promote fix response");
    assert_eq!(promote_fix_resp.status(), StatusCode::OK);
    let promote_fix_payload: Value = serde_json::from_slice(
        &to_bytes(promote_fix_resp.into_body(), usize::MAX)
            .await
            .expect("promote fix body"),
    )
    .expect("promote fix json");
    assert_eq!(
        promote_fix_payload.get("promoted").and_then(Value::as_bool),
        Some(true)
    );

    let fix_hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-promote-fix/memory-hits?q=startup%20fallback%20guard")
        .body(Body::empty())
        .expect("fix hits request");
    let fix_hits_resp = app
        .clone()
        .oneshot(fix_hits_req)
        .await
        .expect("fix hits response");
    assert_eq!(fix_hits_resp.status(), StatusCode::OK);
    let fix_hits_payload: Value = serde_json::from_slice(
        &to_bytes(fix_hits_resp.into_body(), usize::MAX)
            .await
            .expect("fix hits body"),
    )
    .expect("fix hits json");
    let has_promoted_fix_hit = fix_hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str)
                        == promote_fix_payload.get("memory_id").and_then(Value::as_str)
                    && row
                        .get("metadata")
                        .and_then(|metadata| metadata.get("kind"))
                        .and_then(Value::as_str)
                        == Some("fix_pattern")
            })
        })
        .unwrap_or(false);
    assert!(has_promoted_fix_hit);

    let db = super::super::skills_memory::open_global_memory_db()
        .await
        .expect("global memory db");
    let promoted_fix_record = db
        .get_global_memory(
            promote_fix_payload
                .get("memory_id")
                .and_then(Value::as_str)
                .expect("fix memory id"),
        )
        .await
        .expect("load fix governed memory")
        .expect("fix governed memory record");
    assert_eq!(promoted_fix_record.source_type, "solution_capsule");
    assert_eq!(
        promoted_fix_record.project_tag.as_deref(),
        Some("proj-engine")
    );
    assert!(promoted_fix_record.content.contains("workflow: issue_fix"));
    assert!(promoted_fix_record
        .content
        .contains("fix_strategy: add startup fallback guard"));
    assert!(promoted_fix_record
        .content
        .contains("root_cause: Startup recovery skipped the nil-config fallback path."));
    assert_eq!(
        promoted_fix_record
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("kind"))
            .and_then(Value::as_str),
        Some("fix_pattern")
    );
    assert_eq!(
        promoted_fix_record
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_fix")
    );
    assert_eq!(
        promoted_fix_record
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("candidate_id"))
            .and_then(Value::as_str),
        Some(fix_pattern_candidate_id.as_str())
    );
}

#[tokio::test]
async fn coder_issue_triage_reuses_promoted_fix_pattern_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_fix_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-fix-history-a",
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
                    "number": 96,
                    "url": "https://github.com/user123/tandem/issues/96"
                }
            })
            .to_string(),
        ))
        .expect("create fix request");
    let create_fix_resp = app
        .clone()
        .oneshot(create_fix_req)
        .await
        .expect("create fix response");
    assert_eq!(create_fix_resp.status(), StatusCode::OK);

    let fix_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-fix-history-a/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the startup fallback guard and keep the service booting when config is absent.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup fallback regression stays covered"
                }]
            })
            .to_string(),
        ))
        .expect("fix summary request");
    let fix_summary_resp = app
        .clone()
        .oneshot(fix_summary_req)
        .await
        .expect("fix summary response");
    assert_eq!(fix_summary_resp.status(), StatusCode::OK);
    let fix_summary_payload: Value = serde_json::from_slice(
        &to_bytes(fix_summary_resp.into_body(), usize::MAX)
            .await
            .expect("fix summary body"),
    )
    .expect("fix summary json");
    let fix_pattern_candidate_id = fix_summary_payload
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
        .expect("fix pattern candidate id");

    let promote_fix_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-triage-fix-history-a/memory-candidates/{fix_pattern_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-96",
                "reason": "approved reusable fix pattern for future triage"
            })
            .to_string(),
        ))
        .expect("promote fix request");
    let promote_fix_resp = app
        .clone()
        .oneshot(promote_fix_req)
        .await
        .expect("promote fix response");
    assert_eq!(promote_fix_resp.status(), StatusCode::OK);
    let promote_fix_payload: Value = serde_json::from_slice(
        &to_bytes(promote_fix_resp.into_body(), usize::MAX)
            .await
            .expect("promote fix body"),
    )
    .expect("promote fix json");

    let create_triage_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-fix-history-b",
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
                    "number": 96,
                    "url": "https://github.com/user123/tandem/issues/96"
                }
            })
            .to_string(),
        ))
        .expect("create triage request");
    let create_triage_resp = app
        .clone()
        .oneshot(create_triage_req)
        .await
        .expect("create triage response");
    assert_eq!(create_triage_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-triage-fix-history-b/memory-hits")
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
        Some("user123/tandem issue #96")
    );
    assert!(hits_payload
        .get("policy")
        .and_then(|row| row.get("prioritized_kinds"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| row.as_str() == Some("fix_pattern")))
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("memory_id").and_then(Value::as_str)
                == promote_fix_payload.get("memory_id").and_then(Value::as_str)
                && row.get("kind").and_then(Value::as_str) == Some("fix_pattern")
                && row.get("source").and_then(Value::as_str) == Some("governed_memory")
                && row.get("same_issue").and_then(Value::as_bool) == Some(true)
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_promoted_merge_memory_reuses_policy_history_across_pull_requests() {
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
                "coder_run_id": "coder-merge-promote-a",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 101
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
        .uri("/coder/runs/coder-merge-promote-a/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Hold merge until ci / test passes and codeowners approval lands.",
                "risk_level": "medium",
                "blockers": ["Required reviewer approval missing"],
                "required_checks": ["ci / test"],
                "required_approvals": ["codeowners"]
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
    let merge_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory"))
                    .then(|| {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })?
            })
        })
        .expect("merge candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-merge-promote-a/memory-candidates/{merge_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable merge policy memory"
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
                "coder_run_id": "coder-merge-promote-b",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 102
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
        .uri("/coder/runs/coder-merge-promote-b/memory-hits?q=codeowners%20ci%20%2F%20test%20approval")
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
        .expect("promoted merge hit");
    assert_eq!(promoted_hit.get("same_ref").and_then(Value::as_bool), None);
    assert_eq!(
        promoted_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("merge_recommendation_memory")
    );
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("required_checks: ci / test")));
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("required_approvals: codeowners")));
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("blockers: Required reviewer approval missing")));
}

#[tokio::test]
async fn coder_merge_recommendation_memory_promotion_requires_policy_signals() {
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
                "coder_run_id": "coder-merge-promote-policy-guard",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 141
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
        .uri("/coder/runs/coder-merge-promote-policy-guard/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "All signals pass; merge can proceed."
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
    let merge_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory"))
                    .then(|| {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })?
            })
        })
        .expect("merge recommendation candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-merge-promote-policy-guard/memory-candidates/{merge_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "attempted promotion without policy context"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn coder_promoted_merge_outcome_reuses_across_pull_requests() {
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
                "coder_run_id": "coder-merge-outcome-promote-a",
                "workflow_mode": "merge_recommendation",
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
        .uri("/coder/runs/coder-merge-outcome-promote-a/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Merge should wait until rollout notes are attached and post-deploy verification is ready.",
                "risk_level": "medium"
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
            "/coder/runs/coder-merge-outcome-promote-a/memory-candidates/{run_outcome_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable merge outcome"
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
                "coder_run_id": "coder-merge-outcome-promote-b",
                "workflow_mode": "merge_recommendation",
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
        .uri("/coder/runs/coder-merge-outcome-promote-b/memory-hits?q=merge%20should%20wait%20until%20rollout%20notes%20are%20attached")
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
        Some("merge_recommendation")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
}

#[tokio::test]
async fn coder_duplicate_linkage_promotion_requires_linked_issue_and_pr() {
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
                "coder_run_id": "coder-duplicate-linkage-guard",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 991
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

    let create_candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-duplicate-linkage-guard/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "duplicate_linkage",
                "summary": "Link issue to follow-on PR",
                "payload": {
                    "linked_issue_numbers": [991]
                }
            })
            .to_string(),
        ))
        .expect("candidate request");
    let create_candidate_resp = app
        .clone()
        .oneshot(create_candidate_req)
        .await
        .expect("candidate response");
    assert_eq!(create_candidate_resp.status(), StatusCode::OK);
    let create_candidate_payload: Value = serde_json::from_slice(
        &to_bytes(create_candidate_resp.into_body(), usize::MAX)
            .await
            .expect("candidate body"),
    )
    .expect("candidate json");
    let candidate_id = create_candidate_payload
        .get("candidate_id")
        .and_then(Value::as_str)
        .expect("candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-duplicate-linkage-guard/memory-candidates/{candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "attempted reusable duplicate linkage"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn coder_regression_signal_promotion_requires_structured_signals() {
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
                "coder_run_id": "coder-regression-guard",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 992
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

    let create_candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-regression-guard/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "regression_signal",
                "summary": "Historical deploy regression repeated",
                "payload": {
                    "workflow_mode": "pr_review",
                    "summary_artifact_path": "/tmp/fake-summary.json",
                    "regression_signals": []
                }
            })
            .to_string(),
        ))
        .expect("candidate request");
    let create_candidate_resp = app
        .clone()
        .oneshot(create_candidate_req)
        .await
        .expect("candidate response");
    assert_eq!(create_candidate_resp.status(), StatusCode::OK);
    let create_candidate_payload: Value = serde_json::from_slice(
        &to_bytes(create_candidate_resp.into_body(), usize::MAX)
            .await
            .expect("candidate body"),
    )
    .expect("candidate json");
    let candidate_id = create_candidate_payload
        .get("candidate_id")
        .and_then(Value::as_str)
        .expect("candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-regression-guard/memory-candidates/{candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "attempted reusable regression signal"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn coder_terminal_run_outcome_promotion_requires_workflow_evidence() {
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
                "coder_run_id": "coder-run-outcome-guard",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 993
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

    let cancel_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-outcome-guard/cancel")
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
    let cancel_payload: Value = serde_json::from_slice(
        &to_bytes(cancel_resp.into_body(), usize::MAX)
            .await
            .expect("cancel body"),
    )
    .expect("cancel json");
    let candidate_id = cancel_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("candidate_id"))
        .and_then(Value::as_str)
        .expect("run outcome candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-run-outcome-guard/memory-candidates/{candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "attempted reusable cancelled outcome"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::BAD_REQUEST);
}
