use super::*;

#[tokio::test]
async fn coder_issue_triage_run_create_get_and_list() {
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
    assert_eq!(
        artifacts_payload
            .get("artifacts")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
}

#[tokio::test]
async fn coder_issue_triage_blocks_when_github_bindings_are_missing() {
    let state = test_state().await;
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
    assert_eq!(
        artifacts_payload
            .get("artifacts")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        artifacts_payload
            .get("artifacts")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_memory_candidate")
    );
}
