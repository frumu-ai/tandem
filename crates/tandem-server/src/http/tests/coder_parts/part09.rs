
#[tokio::test]
async fn coder_memory_events_include_normalized_artifact_fields() {
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
                "coder_run_id": "coder-run-memory-events",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 335
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
        .uri("/coder/runs/coder-run-memory-events/triage-summary")
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
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
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

    let candidate_event = next_event_of_type(&mut rx, "coder.memory.candidate_added").await;
    assert_eq!(
        candidate_event
            .properties
            .get("kind")
            .and_then(Value::as_str),
        Some("memory_candidate")
    );
    assert_eq!(
        candidate_event
            .properties
            .get("artifact_type")
            .and_then(Value::as_str),
        Some("coder_memory_candidate")
    );
    assert!(candidate_event
        .properties
        .get("artifact_id")
        .and_then(Value::as_str)
        .is_some());
    assert!(candidate_event
        .properties
        .get("artifact_path")
        .and_then(Value::as_str)
        .is_some());

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-run-memory-events/memory-candidates/{triage_candidate_id}/promote"
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

    let promoted_event = next_event_of_type(&mut rx, "coder.memory.promoted").await;
    assert_eq!(
        promoted_event
            .properties
            .get("kind")
            .and_then(Value::as_str),
        Some("memory_promotion")
    );
    assert_eq!(
        promoted_event
            .properties
            .get("artifact_type")
            .and_then(Value::as_str),
        Some("coder_memory_promotion")
    );
    assert!(promoted_event
        .properties
        .get("artifact_id")
        .and_then(Value::as_str)
        .is_some());
    assert!(promoted_event
        .properties
        .get("artifact_path")
        .and_then(Value::as_str)
        .is_some());
}
