use super::*;

#[tokio::test]
async fn pack_builder_preview_external_goal_prefers_mcp_and_generates_mcp_actions() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "preview",
                    "goal": "create a pack that checks latest headline news and posts to slack"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");

    let metadata = payload.get("metadata").cloned().unwrap_or(Value::Null);
    let mapped = metadata
        .get("mcp_mapping")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        mapped.iter().any(|row| {
            row.as_str()
                .is_some_and(|name| name.starts_with("mcp.") && !name.trim().is_empty())
        }),
        "expected at least one MCP tool mapping for external goal"
    );

    let zip_path = metadata
        .get("zip_path")
        .and_then(|v| v.as_str())
        .expect("zip path in preview");
    let file = std::fs::File::open(zip_path).expect("open zip");
    let mut archive = zip::ZipArchive::new(file).expect("zip archive");
    let mut mission = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("missions/default.yaml").expect("mission"),
        &mut mission,
    )
    .expect("read mission");
    assert!(
        mission.lines().any(|line| line.contains("action: mcp.")),
        "mission should explicitly invoke discovered MCP tool IDs"
    );
}

#[tokio::test]
async fn pack_builder_preview_builtin_only_path_does_not_require_connector_selection() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "preview",
                    "auto_apply": false,
                    "goal": "Create a pack that checks latest headline news every day at 8 AM"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let metadata = payload.get("metadata").cloned().unwrap_or(Value::Null);
    assert_eq!(
        metadata
            .get("connector_selection_required")
            .and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        metadata
            .get("selected_connectors")
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(0)
    );
}

#[tokio::test]
async fn pack_builder_preview_auto_applies_when_safe() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "preview",
                    "goal": "Create a pack that checks latest headline news every day at 8 AM"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let metadata = payload.get("metadata").cloned().unwrap_or(Value::Null);
    assert_eq!(
        metadata
            .get("auto_applied_from_preview")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(metadata.get("mode").and_then(|v| v.as_str()), Some("apply"));
    assert!(
        metadata
            .get("pack_installed")
            .and_then(|v| v.get("pack_id"))
            .and_then(|v| v.as_str())
            .is_some(),
        "expected installed pack in auto-apply response"
    );
}

#[tokio::test]
async fn pack_builder_confirmation_goal_applies_last_session_plan() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state);
    let session_id = "session-confirm-flow";

    let preview_req = Request::builder()
            .method("POST")
            .uri("/tool/execute")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tool": "pack_builder",
                    "args": {
                        "__session_id": session_id,
                        "mode": "preview",
                        "auto_apply": false,
                        "goal": "Build a daily technology digest automation that sends a summary to evan@frumu.ai at 8 AM"
                    }
                })
                .to_string(),
            ))
            .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let preview_meta = preview_payload
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Null);
    let expected_plan_id = preview_meta
        .get("plan_id")
        .and_then(|v| v.as_str())
        .expect("preview plan id")
        .to_string();

    let confirm_req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "__session_id": session_id,
                    "mode": "preview",
                    "goal": "ok"
                }
            })
            .to_string(),
        ))
        .expect("confirm request");
    let confirm_resp = app.oneshot(confirm_req).await.expect("confirm response");
    assert_eq!(confirm_resp.status(), StatusCode::OK);
    let confirm_body = to_bytes(confirm_resp.into_body(), usize::MAX)
        .await
        .expect("confirm body");
    let confirm_payload: Value = serde_json::from_slice(&confirm_body).expect("confirm json");
    let confirm_meta = confirm_payload
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Null);

    assert_eq!(
        confirm_meta.get("mode").and_then(|v| v.as_str()),
        Some("apply")
    );
    assert_eq!(
        confirm_meta.get("plan_id").and_then(|v| v.as_str()),
        Some(expected_plan_id.as_str())
    );
    let installed_pack = confirm_meta
        .get("pack_installed")
        .and_then(|v| v.get("pack_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        !installed_pack.ends_with("_ok"),
        "confirmation should apply previous preview plan, not create *_ok pack IDs"
    );
}

#[tokio::test]
async fn pack_builder_apply_requires_explicit_approvals() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state.clone());

    let preview_req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "preview",
                    "goal": "create a pack for notion and slack sync"
                }
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("metadata")
        .and_then(|v| v.get("plan_id"))
        .and_then(|v| v.as_str())
        .expect("plan id");

    let apply_req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "apply",
                    "plan_id": plan_id
                }
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app.oneshot(apply_req).await.expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let metadata = apply_payload
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        metadata.get("error").and_then(|v| v.as_str()),
        Some("approval_required")
    );
}

#[tokio::test]
async fn pack_builder_preview_apply_cancel_pending_endpoints_roundtrip() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state);

    let preview_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-1",
                "thread_key": "web:thread-a",
                "auto_apply": false,
                "goal": "Create a pack to summarize public headline news daily."
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("plan_id")
        .and_then(|v| v.as_str())
        .expect("plan_id")
        .to_string();
    assert_eq!(
        preview_payload.get("status").and_then(|v| v.as_str()),
        Some("preview_pending")
    );

    let pending_req = Request::builder()
        .method("GET")
        .uri("/pack-builder/pending?session_id=pb-session-1&thread_key=web%3Athread-a")
        .body(Body::empty())
        .expect("pending request");
    let pending_resp = app
        .clone()
        .oneshot(pending_req)
        .await
        .expect("pending response");
    assert_eq!(pending_resp.status(), StatusCode::OK);
    let pending_body = to_bytes(pending_resp.into_body(), usize::MAX)
        .await
        .expect("pending body");
    let pending_payload: Value = serde_json::from_slice(&pending_body).expect("pending json");
    assert_eq!(
        pending_payload
            .get("pending")
            .and_then(|v| v.get("plan_id"))
            .and_then(|v| v.as_str()),
        Some(plan_id.as_str())
    );

    let cancel_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/cancel")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-1",
                "thread_key": "web:thread-a",
                "plan_id": plan_id
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
        cancel_payload.get("status").and_then(|v| v.as_str()),
        Some("cancelled")
    );
}

#[tokio::test]
async fn pack_builder_preview_updates_context_blackboard_when_context_run_id_provided() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state);

    let preview_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-bb",
                "thread_key": "web:bb-thread",
                "context_run_id": "ctx-run-pack-builder-1",
                "auto_apply": false,
                "goal": "Create a pack to summarize public headline news daily."
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);

    let blackboard_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-pack-builder-1/blackboard")
        .body(Body::empty())
        .expect("blackboard request");
    let blackboard_resp = app
        .clone()
        .oneshot(blackboard_req)
        .await
        .expect("blackboard response");
    assert_eq!(blackboard_resp.status(), StatusCode::OK);
    let blackboard_body = to_bytes(blackboard_resp.into_body(), usize::MAX)
        .await
        .expect("blackboard body");
    let blackboard_payload: Value =
        serde_json::from_slice(&blackboard_body).expect("blackboard json");
    let tasks = blackboard_payload
        .get("blackboard")
        .and_then(|v| v.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(!tasks.is_empty());
    assert!(tasks.iter().any(|task| {
        task.get("task_type")
            .and_then(Value::as_str)
            .map(|row| row == "pack_builder.preview")
            .unwrap_or(false)
            && task
                .get("workflow_id")
                .and_then(Value::as_str)
                .map(|row| row == "pack_builder")
                .unwrap_or(false)
    }));
}

#[tokio::test]
async fn pack_builder_apply_endpoint_honors_thread_scoped_pending_plan() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state.clone());

    let preview_a = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-threads",
                "thread_key": "thread:a",
                "auto_apply": false,
                "goal": "Create a pack to summarize public headline news daily."
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_a_resp = app
        .clone()
        .oneshot(preview_a)
        .await
        .expect("preview response");
    assert_eq!(preview_a_resp.status(), StatusCode::OK);
    let preview_a_body = to_bytes(preview_a_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_a_payload: Value = serde_json::from_slice(&preview_a_body).expect("preview json");
    let plan_thread_a = preview_a_payload
        .get("plan_id")
        .and_then(|v| v.as_str())
        .expect("plan id")
        .to_string();

    let preview_b = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-threads",
                "thread_key": "thread:b",
                "auto_apply": false,
                "goal": "Create a pack to summarize public headline news daily."
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_b_resp = app
        .clone()
        .oneshot(preview_b)
        .await
        .expect("preview response");
    assert_eq!(preview_b_resp.status(), StatusCode::OK);

    let apply_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-threads",
                "thread_key": "thread:a",
                "approvals": {
                    "approve_pack_install": true,
                    "approve_connector_registration": true,
                    "approve_enable_routines": false
                },
                "secret_refs_confirmed": true
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app.oneshot(apply_req).await.expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automations_registered = apply_payload["automations_registered"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let routines_registered = apply_payload["routines_registered"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(automations_registered.len(), routines_registered.len());
    if let Some(automation_id) = automations_registered.first().and_then(Value::as_str) {
        let automation = state
            .get_automation_v2(automation_id)
            .await
            .expect("stored pack builder automation");
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("origin"))
                .and_then(|v| v.as_str()),
            Some("pack_builder")
        );
        assert_eq!(automation.status, crate::AutomationV2Status::Paused);
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("activation_mode"))
                .and_then(|v| v.as_str()),
            Some("routine_wrapper_mirror")
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("pack_builder_plan_id"))
                .and_then(|v| v.as_str()),
            Some(plan_thread_a.as_str())
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("routine_id"))
                .and_then(|v| v.as_str()),
            routines_registered[0].as_str()
        );
    }
}

#[tokio::test]
async fn pack_builder_apply_endpoint_blocks_when_required_secrets_missing() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
    let app = app_router(state);

    let preview_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-secrets",
                "thread_key": "thread:secrets",
                "auto_apply": false,
                "goal": "Create a pack that syncs Notion updates to Slack every day"
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let required = preview_payload
        .get("required_secrets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if required.is_empty() {
        return;
    }

    let apply_req = Request::builder()
            .method("POST")
            .uri("/pack-builder/apply")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "session_id": "pb-session-secrets",
                    "thread_key": "thread:secrets",
                    "plan_id": preview_payload.get("plan_id").and_then(|v| v.as_str()).unwrap_or_default(),
                    "approvals": {
                        "approve_pack_install": true,
                        "approve_connector_registration": true,
                        "approve_enable_routines": false
                    },
                    "secret_refs_confirmed": false
                })
                .to_string(),
            ))
            .expect("apply request");
    let apply_resp = app.oneshot(apply_req).await.expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    assert_eq!(
        apply_payload.get("status").and_then(|v| v.as_str()),
        Some("apply_blocked_missing_secrets")
    );
}
