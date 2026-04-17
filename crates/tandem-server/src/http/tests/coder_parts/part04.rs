
#[tokio::test]
async fn coder_merge_recommendation_summary_ready_to_merge_awaits_approval() {
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
                "coder_run_id": "coder-merge-ready-for-approval",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 193
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
        .uri("/coder/runs/coder-merge-ready-for-approval/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "All required checks and approvals are satisfied.",
                "risk_level": "low",
                "blockers": [],
                "required_checks": [],
                "required_approvals": [],
                "memory_hits_used": ["memory-hit-merge-ready-1"],
                "notes": "Ready for operator approval."
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
            .get("approval_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("awaiting_approval")
    );
    assert_eq!(
        summary_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("approval")
    );

    let approval_event = next_event_of_type(&mut rx, "coder.approval.required").await;
    assert_eq!(
        approval_event
            .properties
            .get("event_type")
            .and_then(Value::as_str),
        Some("merge_recommendation_ready")
    );
    assert_eq!(
        approval_event
            .properties
            .get("recommendation")
            .and_then(Value::as_str),
        Some("merge")
    );
    assert_eq!(
        approval_event
            .properties
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_policy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        approval_event
            .properties
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_block_reason"))
            .and_then(Value::as_str),
        Some("requires_merge_execution_request")
    );

    let approve_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-ready-for-approval/approve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "Operator approved the merge recommendation."
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
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        approve_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        approve_payload
            .get("event")
            .and_then(|row| row.get("type"))
            .and_then(Value::as_str),
        Some("merge_recommendation_approved")
    );
    assert_eq!(
        approve_payload
            .get("merge_execution_request")
            .and_then(|row| row.get("recommendation"))
            .and_then(Value::as_str),
        Some("merge")
    );
    assert!(approve_payload
        .get("worker_run_reference")
        .is_some_and(Value::is_null));
    assert!(approve_payload
        .get("worker_session_context_run_id")
        .is_some_and(Value::is_null));
    assert!(approve_payload
        .get("validation_run_reference")
        .is_some_and(Value::is_null));
    assert!(approve_payload
        .get("validation_session_context_run_id")
        .is_some_and(Value::is_null));
    assert_eq!(
        approve_payload
            .get("merge_execution_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_merge_execution_request")
    );
    let merge_execution_artifact_path = approve_payload
        .get("merge_execution_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .expect("merge execution artifact path");
    let merge_execution_artifact_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(merge_execution_artifact_path)
            .await
            .expect("read merge execution artifact"),
    )
    .expect("parse merge execution artifact");
    assert_eq!(
        merge_execution_artifact_payload
            .get("merge_submit_policy_preview")
            .and_then(|row| row.get("preferred_submit_mode"))
            .and_then(Value::as_str),
        Some("manual")
    );
    assert_eq!(
        merge_execution_artifact_payload
            .get("merge_submit_policy_preview")
            .and_then(|row| row.get("explicit_submit_required"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        merge_execution_artifact_payload
            .get("merge_submit_policy_preview")
            .and_then(|row| row.get("auto_execute_after_approval"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        merge_execution_artifact_payload
            .get("merge_submit_policy_preview")
            .and_then(|row| row.get("auto_execute_eligible"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        merge_execution_artifact_payload
            .get("merge_submit_policy_preview")
            .and_then(|row| row.get("auto_execute_policy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        merge_execution_artifact_payload
            .get("merge_submit_policy_preview")
            .and_then(|row| row.get("auto_execute_block_reason"))
            .and_then(Value::as_str),
        Some("project_auto_merge_policy_disabled")
    );
    assert_eq!(
        merge_execution_artifact_payload
            .get("merge_submit_policy_preview")
            .and_then(|row| row.get("manual"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(false)
    );
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
            .and_then(|row| row.get("auto_execute_after_approval"))
            .and_then(Value::as_bool),
        Some(false)
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
        Some("project_auto_merge_policy_disabled")
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("manual"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto"))
            .and_then(|row| row.get("policy"))
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_explicit_auto_merge_submit_opt_in")
    );

    let merge_event = next_event_of_type(&mut rx, "coder.merge.recommended").await;
    assert_eq!(
        merge_event
            .properties
            .get("event_type")
            .and_then(Value::as_str),
        Some("merge_execution_request_ready")
    );
    assert_eq!(
        merge_event
            .properties
            .get("recommendation")
            .and_then(Value::as_str),
        Some("merge")
    );
    assert_eq!(
        merge_event
            .properties
            .get("merge_submit_policy")
            .and_then(|row| row.get("preferred_submit_mode"))
            .and_then(Value::as_str),
        Some("manual")
    );
    assert_eq!(
        merge_event
            .properties
            .get("merge_submit_policy")
            .and_then(|row| row.get("explicit_submit_required"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        merge_event
            .properties
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_eligible"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        merge_event
            .properties
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn coder_merge_submit_real_submit_writes_merge_artifact() {
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
                "coder_run_id": "coder-merge-submit-real",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 314
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
        .uri("/coder/runs/coder-merge-submit-real/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "Checks and approvals are complete.",
                "risk_level": "low",
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
        .uri("/coder/runs/coder-merge-submit-real/approve")
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
        .uri("/coder/runs/coder-merge-submit-real/merge-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Execute the approved merge",
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
        submit_payload.get("submitted").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        submit_payload
            .get("merged_github_ref")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        submit_payload
            .get("merge_result")
            .and_then(|row| row.get("merged"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        submit_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_merge_submission")
    );
    assert_eq!(
        submit_payload
            .get("external_action")
            .and_then(|row| row.get("capability_id"))
            .and_then(Value::as_str),
        Some("github.merge_pull_request")
    );
    assert_eq!(
        submit_payload
            .get("external_action")
            .and_then(|row| row.get("source_kind"))
            .and_then(Value::as_str),
        Some("coder")
    );
    assert!(submit_payload
        .get("worker_run_reference")
        .is_some_and(Value::is_null));
    assert!(submit_payload
        .get("worker_session_context_run_id")
        .is_some_and(Value::is_null));
    assert!(submit_payload
        .get("validation_run_reference")
        .is_some_and(Value::is_null));
    assert!(submit_payload
        .get("validation_session_context_run_id")
        .is_some_and(Value::is_null));

    let merge_submit_event = next_event_of_type(&mut rx, "coder.merge.submitted").await;
    assert_eq!(
        merge_submit_event
            .properties
            .get("merged_github_ref")
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
}

#[tokio::test]
async fn coder_project_policy_get_and_put_controls_auto_merge_flag() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let get_before_req = Request::builder()
        .method("GET")
        .uri("/coder/projects/proj-engine/policy")
        .body(Body::empty())
        .expect("get before request");
    let get_before_resp = app
        .clone()
        .oneshot(get_before_req)
        .await
        .expect("get before response");
    assert_eq!(get_before_resp.status(), StatusCode::OK);
    let get_before_payload: Value = serde_json::from_slice(
        &to_bytes(get_before_resp.into_body(), usize::MAX)
            .await
            .expect("get before body"),
    )
    .expect("get before json");
    assert_eq!(
        get_before_payload
            .get("project_policy")
            .and_then(|row| row.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-engine")
    );
    assert_eq!(
        get_before_payload
            .get("project_policy")
            .and_then(|row| row.get("auto_merge_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );

    let put_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/policy")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "auto_merge_enabled": true
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_payload: Value = serde_json::from_slice(
        &to_bytes(put_resp.into_body(), usize::MAX)
            .await
            .expect("put body"),
    )
    .expect("put json");
    assert_eq!(put_payload.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        put_payload
            .get("project_policy")
            .and_then(|row| row.get("auto_merge_enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let get_after_req = Request::builder()
        .method("GET")
        .uri("/coder/projects/proj-engine/policy")
        .body(Body::empty())
        .expect("get after request");
    let get_after_resp = app
        .clone()
        .oneshot(get_after_req)
        .await
        .expect("get after response");
    assert_eq!(get_after_resp.status(), StatusCode::OK);
    let get_after_payload: Value = serde_json::from_slice(
        &to_bytes(get_after_resp.into_body(), usize::MAX)
            .await
            .expect("get after body"),
    )
    .expect("get after json");
    assert_eq!(
        get_after_payload
            .get("project_policy")
            .and_then(|row| row.get("auto_merge_enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn coder_project_list_summarizes_known_repo_bindings_and_policy() {
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

    for payload in [
        json!({
            "coder_run_id": "coder-project-list-a",
            "workflow_mode": "issue_triage",
            "repo_binding": {
                "project_id": "proj-engine",
                "workspace_id": "ws-tandem",
                "workspace_root": "/tmp/tandem-repo",
                "repo_slug": "user123/tandem"
            },
            "github_ref": {
                "kind": "issue",
                "number": 321
            },
            "mcp_servers": ["github"]
        }),
        json!({
            "coder_run_id": "coder-project-list-b",
            "workflow_mode": "issue_fix",
            "repo_binding": {
                "project_id": "proj-engine",
                "workspace_id": "ws-tandem",
                "workspace_root": "/tmp/tandem-repo",
                "repo_slug": "user123/tandem"
            },
            "github_ref": {
                "kind": "issue",
                "number": 322
            },
            "mcp_servers": ["github"]
        }),
        json!({
            "coder_run_id": "coder-project-list-c",
            "workflow_mode": "pr_review",
            "repo_binding": {
                "project_id": "proj-docs",
                "workspace_id": "ws-docs",
                "workspace_root": "/tmp/docs-repo",
                "repo_slug": "user123/docs"
            },
            "github_ref": {
                "kind": "pull_request",
                "number": 12
            },
            "mcp_servers": ["github"]
        }),
    ] {
        let create_req = Request::builder()
            .method("POST")
            .uri("/coder/runs")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
    }

    let list_req = Request::builder()
        .method("GET")
        .uri("/coder/projects")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    server.abort();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_payload: Value = serde_json::from_slice(
        &to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list body"),
    )
    .expect("list json");
    let projects = list_payload
        .get("projects")
        .and_then(Value::as_array)
        .expect("projects array");
    assert_eq!(projects.len(), 2);
    let engine_project = projects
        .iter()
        .find(|row| row.get("project_id").and_then(Value::as_str) == Some("proj-engine"))
        .expect("engine project");
    let docs_project = projects
        .iter()
        .find(|row| row.get("project_id").and_then(Value::as_str) == Some("proj-docs"))
        .expect("docs project");
    assert_eq!(
        engine_project.get("project_id").and_then(Value::as_str),
        Some("proj-engine")
    );
    assert_eq!(
        engine_project.get("run_count").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        engine_project
            .get("project_policy")
            .and_then(|row| row.get("auto_merge_enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        engine_project
            .get("workflow_modes")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["issue_fix", "issue_triage"])
    );
    assert_eq!(
        docs_project.get("project_id").and_then(Value::as_str),
        Some("proj-docs")
    );
    assert_eq!(
        docs_project
            .get("project_policy")
            .and_then(|row| row.get("auto_merge_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[tokio::test]
async fn coder_project_binding_get_put_and_project_list_prefers_explicit_binding() {
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

    let put_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/bindings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "project_id": "ignored-by-endpoint",
                "workspace_id": "ws-explicit",
                "workspace_root": "/tmp/explicit-repo",
                "repo_slug": "user123/tandem-explicit"
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_payload: Value = serde_json::from_slice(
        &to_bytes(put_resp.into_body(), usize::MAX)
            .await
            .expect("put body"),
    )
    .expect("put json");
    assert_eq!(
        put_payload
            .get("binding")
            .and_then(|row| row.get("repo_binding"))
            .and_then(|row| row.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-engine")
    );

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/projects/proj-engine/bindings")
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
            .get("binding")
            .and_then(|row| row.get("repo_binding"))
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("user123/tandem-explicit")
    );

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-project-binding-run",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-derived",
                    "workspace_root": "/tmp/derived-repo",
                    "repo_slug": "user123/tandem-derived"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 325
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

    let list_req = Request::builder()
        .method("GET")
        .uri("/coder/projects")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    server.abort();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_payload: Value = serde_json::from_slice(
        &to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list body"),
    )
    .expect("list json");
    let engine_project = list_payload
        .get("projects")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("project_id").and_then(Value::as_str) == Some("proj-engine"))
        })
        .expect("engine project");
    assert_eq!(
        engine_project
            .get("repo_binding")
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("user123/tandem-explicit")
    );
    assert_eq!(
        engine_project
            .get("repo_binding")
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/explicit-repo")
    );
}

#[tokio::test]
async fn coder_project_binding_put_bootstraps_github_mcp_server_from_auth() {
    let (endpoint, server) = spawn_fake_github_mcp_server().await;
    let state = test_state().await;
    assert!(state.mcp.remove("github").await);
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    assert!(
        super::super::ensure_remote_mcp_server(
            &state,
            "github",
            &endpoint,
            std::collections::HashMap::from([(
                "Authorization".to_string(),
                "Bearer test-token".to_string(),
            )]),
        )
        .await
    );
    let app = app_router(state.clone());

    let put_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/bindings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_binding": {
                    "workspace_id": "ws-explicit",
                    "workspace_root": "/tmp/explicit-repo",
                    "repo_slug": "user123/tandem-explicit"
                },
                "github_project_binding": {
                    "owner": "user123",
                    "project_number": 42
                }
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    server.abort();
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_payload: Value = serde_json::from_slice(
        &to_bytes(put_resp.into_body(), usize::MAX)
            .await
            .expect("put body"),
    )
    .expect("put json");
    assert_eq!(
        put_payload
            .get("binding")
            .and_then(|row| row.get("github_project_binding"))
            .and_then(|row| row.get("mcp_server"))
            .and_then(Value::as_str),
        Some("github")
    );
}

#[tokio::test]
async fn coder_project_binding_put_discovers_github_project_schema() {
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

    let put_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/bindings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_binding": {
                    "workspace_id": "ws-explicit",
                    "workspace_root": "/tmp/explicit-repo",
                    "repo_slug": "user123/tandem-explicit"
                },
                "github_project_binding": {
                    "owner": "user123",
                    "project_number": 42,
                    "mcp_server": "github"
                }
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    server.abort();
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_payload: Value = serde_json::from_slice(
        &to_bytes(put_resp.into_body(), usize::MAX)
            .await
            .expect("put body"),
    )
    .expect("put json");
    assert_eq!(
        put_payload
            .get("binding")
            .and_then(|row| row.get("github_project_binding"))
            .and_then(|row| row.get("schema_fingerprint"))
            .and_then(Value::as_str)
            .map(|row| !row.is_empty()),
        Some(true)
    );
    assert_eq!(
        put_payload
            .get("binding")
            .and_then(|row| row.get("github_project_binding"))
            .and_then(|row| row.get("status_mapping"))
            .and_then(|row| row.get("todo"))
            .and_then(|row| row.get("name"))
            .and_then(Value::as_str),
        Some("TODO")
    );
}

#[tokio::test]
async fn coder_project_github_project_inbox_lists_actionable_and_unsupported_items() {
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

    let binding_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/bindings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_binding": {
                    "workspace_id": "ws-explicit",
                    "workspace_root": "/tmp/explicit-repo",
                    "repo_slug": "user123/tandem-explicit"
                },
                "github_project_binding": {
                    "owner": "user123",
                    "project_number": 42,
                    "mcp_server": "github"
                }
            })
            .to_string(),
        ))
        .expect("binding request");
    let binding_resp = app
        .clone()
        .oneshot(binding_req)
        .await
        .expect("binding response");
    assert_eq!(binding_resp.status(), StatusCode::OK);

    let inbox_req = Request::builder()
        .method("GET")
        .uri("/coder/projects/proj-engine/github-project/inbox")
        .body(Body::empty())
        .expect("inbox request");
    let inbox_resp = app
        .clone()
        .oneshot(inbox_req)
        .await
        .expect("inbox response");
    server.abort();
    assert_eq!(inbox_resp.status(), StatusCode::OK);
    let inbox_payload: Value = serde_json::from_slice(
        &to_bytes(inbox_resp.into_body(), usize::MAX)
            .await
            .expect("inbox body"),
    )
    .expect("inbox json");
    let items = inbox_payload
        .get("items")
        .and_then(Value::as_array)
        .expect("items");
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].get("actionable").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        items[1].get("unsupported_reason").and_then(Value::as_str),
        Some("unsupported_item_type")
    );
}

#[tokio::test]
async fn coder_project_github_project_intake_is_idempotent_for_active_item() {
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

    let binding_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/bindings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_binding": {
                    "workspace_id": "ws-explicit",
                    "workspace_root": "/tmp/explicit-repo",
                    "repo_slug": "user123/tandem-explicit"
                },
                "github_project_binding": {
                    "owner": "user123",
                    "project_number": 42,
                    "mcp_server": "github"
                }
            })
            .to_string(),
        ))
        .expect("binding request");
    let binding_resp = app
        .clone()
        .oneshot(binding_req)
        .await
        .expect("binding response");
    assert_eq!(binding_resp.status(), StatusCode::OK);

    let first_req = Request::builder()
        .method("POST")
        .uri("/coder/projects/proj-engine/github-project/intake")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "project_item_id": "PVT_item_1",
                "mcp_servers": ["github"]
            })
            .to_string(),
        ))
        .expect("first intake request");
    let first_resp = app
        .clone()
        .oneshot(first_req)
        .await
        .expect("first intake response");
    assert_eq!(first_resp.status(), StatusCode::OK);
    let first_payload: Value = serde_json::from_slice(
        &to_bytes(first_resp.into_body(), usize::MAX)
            .await
            .expect("first intake body"),
    )
    .expect("first intake json");
    let first_run_id = first_payload
        .get("coder_run")
        .and_then(|row| row.get("coder_run_id"))
        .and_then(Value::as_str)
        .expect("first coder run id")
        .to_string();
    assert_eq!(
        first_payload
            .get("coder_run")
            .and_then(|row| row.get("github_project_ref"))
            .and_then(|row| row.get("project_item_id"))
            .and_then(Value::as_str),
        Some("PVT_item_1")
    );

    let second_req = Request::builder()
        .method("POST")
        .uri("/coder/projects/proj-engine/github-project/intake")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "project_item_id": "PVT_item_1",
                "mcp_servers": ["github"]
            })
            .to_string(),
        ))
        .expect("second intake request");
    let second_resp = app
        .clone()
        .oneshot(second_req)
        .await
        .expect("second intake response");
    server.abort();
    assert_eq!(second_resp.status(), StatusCode::OK);
    let second_payload: Value = serde_json::from_slice(
        &to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .expect("second intake body"),
    )
    .expect("second intake json");
    assert_eq!(
        second_payload.get("deduped").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        second_payload
            .get("coder_run")
            .and_then(|row| row.get("coder_run_id"))
            .and_then(Value::as_str),
        Some(first_run_id.as_str())
    );
}

#[tokio::test]
async fn coder_project_get_returns_policy_binding_and_recent_runs() {
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

    let binding_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/bindings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "project_id": "ignored-by-endpoint",
                "workspace_id": "ws-explicit",
                "workspace_root": "/tmp/explicit-repo",
                "repo_slug": "user123/tandem-explicit",
                "default_branch": "main"
            })
            .to_string(),
        ))
        .expect("binding request");
    let binding_resp = app
        .clone()
        .oneshot(binding_req)
        .await
        .expect("binding response");
    assert_eq!(binding_resp.status(), StatusCode::OK);

    for (coder_run_id, workflow_mode, number) in [
        ("coder-project-detail-triage", "issue_triage", 41_u64),
        ("coder-project-detail-fix", "issue_fix", 42_u64),
    ] {
        let create_req = Request::builder()
            .method("POST")
            .uri("/coder/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "coder_run_id": coder_run_id,
                    "workflow_mode": workflow_mode,
                    "repo_binding": {
                        "project_id": "proj-engine",
                        "workspace_id": "ws-derived",
                        "workspace_root": "/tmp/derived-repo",
                        "repo_slug": "user123/tandem-derived"
                    },
                    "github_ref": {
                        "kind": "issue",
                        "number": number
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
    }

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/projects/proj-engine")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    server.abort();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_payload: Value = serde_json::from_slice(
        &to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body"),
    )
    .expect("get json");
    assert_eq!(
        get_payload
            .get("project")
            .and_then(|row| row.get("repo_binding"))
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("user123/tandem-explicit")
    );
    assert_eq!(
        get_payload
            .get("binding")
            .and_then(|row| row.get("repo_binding"))
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/explicit-repo")
    );
    assert_eq!(
        get_payload
            .get("project_policy")
            .and_then(|row| row.get("auto_merge_enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        get_payload
            .get("project")
            .and_then(|row| row.get("run_count"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        get_payload
            .get("project")
            .and_then(|row| row.get("workflow_modes"))
            .cloned(),
        Some(json!(["issue_fix", "issue_triage"]))
    );
    let recent_runs = get_payload
        .get("recent_runs")
        .and_then(Value::as_array)
        .expect("recent runs");
    assert_eq!(recent_runs.len(), 2);
    assert_eq!(
        recent_runs
            .first()
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_fix")
    );
    assert!(
        recent_runs
            .first()
            .and_then(|row| row.get("execution_policy"))
            .map(Value::is_object)
            .unwrap_or(false),
        "expected execution policy on recent run"
    );
}

#[tokio::test]
async fn coder_project_run_create_uses_saved_binding_and_requires_it() {
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

    let missing_binding_req = Request::builder()
        .method("POST")
        .uri("/coder/projects/proj-missing/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "issue_triage",
                "github_ref": {
                    "kind": "issue",
                    "number": 7
                },
                "mcp_servers": ["github"]
            })
            .to_string(),
        ))
        .expect("missing binding request");
    let missing_binding_resp = app
        .clone()
        .oneshot(missing_binding_req)
        .await
        .expect("missing binding response");
    assert_eq!(missing_binding_resp.status(), StatusCode::CONFLICT);
    let missing_binding_payload: Value = serde_json::from_slice(
        &to_bytes(missing_binding_resp.into_body(), usize::MAX)
            .await
            .expect("missing binding body"),
    )
    .expect("missing binding json");
    assert_eq!(
        missing_binding_payload.get("code").and_then(Value::as_str),
        Some("CODER_PROJECT_BINDING_REQUIRED")
    );

    let binding_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/bindings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "project_id": "ignored-by-endpoint",
                "workspace_id": "ws-explicit",
                "workspace_root": "/tmp/explicit-repo",
                "repo_slug": "user123/tandem-explicit",
                "default_branch": "main"
            })
            .to_string(),
        ))
        .expect("binding request");
    let binding_resp = app
        .clone()
        .oneshot(binding_req)
        .await
        .expect("binding response");
    assert_eq!(binding_resp.status(), StatusCode::OK);

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/projects/proj-engine/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-project-scoped-run",
                "workflow_mode": "issue_triage",
                "github_ref": {
                    "kind": "issue",
                    "number": 91
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
    server.abort();
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    assert_eq!(
        create_payload
            .get("coder_run")
            .and_then(|row| row.get("repo_binding"))
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("user123/tandem-explicit")
    );
    assert_eq!(
        create_payload
            .get("coder_run")
            .and_then(|row| row.get("repo_binding"))
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/explicit-repo")
    );
}

#[tokio::test]
async fn coder_project_run_list_filters_to_project_and_sorts_newest_first() {
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

    for (coder_run_id, project_id, workflow_mode, number) in [
        (
            "coder-project-runs-triage",
            "proj-engine",
            "issue_triage",
            51_u64,
        ),
        ("coder-project-runs-fix", "proj-engine", "issue_fix", 52_u64),
        (
            "coder-project-runs-review",
            "proj-other",
            "pr_review",
            53_u64,
        ),
    ] {
        let kind = if workflow_mode == "pr_review" {
            "pull_request"
        } else {
            "issue"
        };
        let create_req = Request::builder()
            .method("POST")
            .uri("/coder/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "coder_run_id": coder_run_id,
                    "workflow_mode": workflow_mode,
                    "repo_binding": {
                        "project_id": project_id,
                        "workspace_id": format!("ws-{project_id}"),
                        "workspace_root": format!("/tmp/{project_id}"),
                        "repo_slug": format!("user123/{project_id}")
                    },
                    "github_ref": {
                        "kind": kind,
                        "number": number
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
    }

    let list_req = Request::builder()
        .method("GET")
        .uri("/coder/projects/proj-engine/runs?limit=10")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    server.abort();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_payload: Value = serde_json::from_slice(
        &to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list body"),
    )
    .expect("list json");
    assert_eq!(
        list_payload.get("project_id").and_then(Value::as_str),
        Some("proj-engine")
    );
    let runs = list_payload
        .get("runs")
        .and_then(Value::as_array)
        .expect("runs");
    assert_eq!(runs.len(), 2);
    assert_eq!(
        runs.first()
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_fix")
    );
    assert_eq!(
        runs.get(1)
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert!(
        runs.iter().all(|row| {
            row.get("coder_run")
                .and_then(|coder_run| coder_run.get("repo_binding"))
                .and_then(|binding| binding.get("project_id"))
                .and_then(Value::as_str)
                == Some("proj-engine")
        }),
        "expected only proj-engine runs"
    );
}

#[tokio::test]
async fn coder_status_summarizes_active_and_approval_runs() {
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

    for payload in [
        json!({
            "coder_run_id": "coder-status-issue-fix",
            "workflow_mode": "issue_fix",
            "repo_binding": {
                "project_id": "proj-engine",
                "workspace_id": "ws-tandem",
                "workspace_root": "/tmp/tandem-repo",
                "repo_slug": "user123/tandem"
            },
            "github_ref": {
                "kind": "issue",
                "number": 323
            },
            "mcp_servers": ["github"]
        }),
        json!({
            "coder_run_id": "coder-status-merge",
            "workflow_mode": "merge_recommendation",
            "repo_binding": {
                "project_id": "proj-engine",
                "workspace_id": "ws-tandem",
                "workspace_root": "/tmp/tandem-repo",
                "repo_slug": "user123/tandem"
            },
            "github_ref": {
                "kind": "pull_request",
                "number": 324
            },
            "mcp_servers": ["github"]
        }),
    ] {
        let create_req = Request::builder()
            .method("POST")
            .uri("/coder/runs")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
    }

    let merge_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-status-merge/merge-recommendation-summary")
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

    let status_req = Request::builder()
        .method("GET")
        .uri("/coder/status")
        .body(Body::empty())
        .expect("status request");
    let status_resp = app
        .clone()
        .oneshot(status_req)
        .await
        .expect("status response");
    server.abort();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_payload: Value = serde_json::from_slice(
        &to_bytes(status_resp.into_body(), usize::MAX)
            .await
            .expect("status body"),
    )
    .expect("status json");
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("total_runs"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("active_runs"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("awaiting_approval_runs"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("project_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("workflow_counts"))
            .and_then(|row| row.get("issue_fix"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("workflow_counts"))
            .and_then(|row| row.get("merge_recommendation"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("run_status_counts"))
            .and_then(|row| row.get("running"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("run_status_counts"))
            .and_then(|row| row.get("awaiting_approval"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status_payload
            .get("status")
            .and_then(|row| row.get("latest_run"))
            .and_then(|row| row.get("coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-status-merge")
    );
}
