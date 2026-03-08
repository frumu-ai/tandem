use super::*;

#[tokio::test]
async fn workflow_plan_preview_returns_normalized_plan() {
    let state = test_state().await;
    let snapshot_root = state.workspace_index.snapshot().await.root;
    let expected_workspace_root = crate::normalize_absolute_workspace_root(&snapshot_root)
        .or_else(|_| {
            std::env::current_dir()
                .map_err(|_| "workspace_root must be an absolute path".to_string())
                .and_then(|cwd| {
                    crate::normalize_absolute_workspace_root(cwd.to_string_lossy().as_ref())
                })
        })
        .expect("expected workspace root");
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Every morning research market pain points and write a report",
                "plan_source": "automations_page"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let plan = payload.get("plan").expect("plan");
    assert_eq!(
        plan.get("execution_target").and_then(Value::as_str),
        Some("automation_v2")
    );
    assert_eq!(plan.get("confidence").and_then(Value::as_str), Some("high"));
    assert_eq!(
        plan.get("workspace_root").and_then(Value::as_str),
        Some(expected_workspace_root.as_str())
    );
    assert!(plan
        .get("steps")
        .and_then(Value::as_array)
        .is_some_and(|steps| steps.len() >= 2));
}

#[tokio::test]
async fn workflow_plan_preview_rejects_relative_workspace_root() {
    let state = test_state().await;
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Research the market",
                "workspace_root": "relative/path"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("WORKFLOW_PLAN_INVALID")
    );
}

#[tokio::test]
async fn workflow_plan_apply_persists_automation_v2_with_planner_metadata() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let preview_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Compare two competitor summaries and generate a report",
                "plan_source": "automations_page",
                "allowed_mcp_servers": ["slack", "github", "github"],
                "workspace_root": "/tmp/custom-workspace",
                "operator_preferences": {
                    "execution_mode": "swarm",
                    "max_parallel_agents": 6,
                    "model_provider": "openai",
                    "model_id": "gpt-5.1",
                    "role_models": {
                        "planner": {
                            "provider_id": "anthropic",
                            "model_id": "claude-sonnet-4"
                        }
                    }
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
        .get("plan")
        .and_then(|plan| plan.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan_id")
        .to_string();

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation_id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert_eq!(stored.creator_id, "control-panel");
    assert_eq!(
        stored.workspace_root.as_deref(),
        Some("/tmp/custom-workspace")
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("planner_version"))
            .and_then(Value::as_str),
        Some("v1")
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_source"))
            .and_then(Value::as_str),
        Some("automations_page")
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/custom-workspace")
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("allowed_mcp_servers"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["github".to_string(), "slack".to_string()])
    );
    assert!(stored
        .agents
        .iter()
        .all(|agent| agent.mcp_policy.allowed_servers
            == vec!["github".to_string(), "slack".to_string()]));
    assert_eq!(stored.execution.max_parallel_agents, Some(6));
    assert!(stored.agents.iter().all(|agent| {
        agent
            .model_policy
            .as_ref()
            .and_then(|policy| policy.get("default_model"))
            .and_then(|row| row.get("provider_id"))
            .and_then(Value::as_str)
            == Some("openai")
            && agent
                .model_policy
                .as_ref()
                .and_then(|policy| policy.get("default_model"))
                .and_then(|row| row.get("model_id"))
                .and_then(Value::as_str)
                == Some("gpt-5.1")
    }));
    assert!(stored.agents.iter().all(|agent| {
        agent
            .model_policy
            .as_ref()
            .and_then(|policy| policy.get("role_models"))
            .and_then(|row| row.get("planner"))
            .and_then(|row| row.get("provider_id"))
            .and_then(Value::as_str)
            == Some("anthropic")
    }));
    assert!(stored
        .flow
        .nodes
        .iter()
        .any(|node| !node.input_refs.is_empty()));
    assert!(stored
        .flow
        .nodes
        .iter()
        .any(|node| node.output_contract.is_some()));
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("operator_preferences"))
            .and_then(|row| row.get("execution_mode"))
            .and_then(Value::as_str),
        Some("swarm")
    );
}

#[tokio::test]
async fn workflow_plan_apply_can_export_to_pack_builder_preview() {
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
        .uri("/workflow-plans/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Create a daily digest for competitor issue updates",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/repo"
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
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel",
                "pack_builder_export": {
                    "enabled": true,
                    "session_id": "wf-plan-export-session",
                    "thread_key": "wf-plan-export-thread",
                    "auto_apply": false
                }
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let export_payload = apply_payload
        .get("pack_builder_export")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        export_payload.get("status").and_then(Value::as_str),
        Some("preview_pending")
    );
    let exported_plan_id = export_payload
        .get("plan_id")
        .and_then(Value::as_str)
        .expect("pack builder plan id")
        .to_string();

    let pending_req = Request::builder()
        .method("GET")
        .uri("/pack-builder/pending?session_id=wf-plan-export-session&thread_key=wf-plan-export-thread")
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
            .and_then(|row| row.get("plan_id"))
            .and_then(Value::as_str),
        Some(exported_plan_id.as_str())
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_revises_draft_and_reset_restores_initial_plan() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Send me a daily competitor digest",
                "plan_source": "automations_page",
                "allowed_mcp_servers": ["github", "slack"],
                "workspace_root": "/tmp/initial-workspace"
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Make this weekly, run it from /tmp/revised-workspace, and remove slack."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/revised-workspace")
    );
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("allowed_mcp_servers"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["github".to_string()])
    );

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/workflow-plans/{plan_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);

    let reset_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/reset")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id
            })
            .to_string(),
        ))
        .expect("reset request");
    let reset_resp = app
        .clone()
        .oneshot(reset_req)
        .await
        .expect("reset response");
    assert_eq!(reset_resp.status(), StatusCode::OK);
    let reset_body = to_bytes(reset_resp.into_body(), usize::MAX)
        .await
        .expect("reset body");
    let reset_payload: Value = serde_json::from_slice(&reset_body).expect("reset json");
    assert_eq!(
        reset_payload
            .get("plan")
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/initial-workspace")
    );
    let draft = state
        .get_workflow_plan_draft(&plan_id)
        .await
        .expect("draft");
    assert!(draft.conversation.messages.len() >= 3);
}

#[tokio::test]
async fn workflow_plan_chat_message_returns_clarifier_for_invalid_workspace_root_revision() {
    let state = test_state().await;
    let app = app_router(state);

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Send me a daily competitor digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace"
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Run it from relative/path instead."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/initial-workspace")
    );
    assert_eq!(
        message_payload
            .get("clarifier")
            .and_then(|row| row.get("field"))
            .and_then(Value::as_str),
        Some("workspace_root")
    );
    assert_eq!(
        message_payload
            .get("clarifier")
            .and_then(|row| row.get("question"))
            .and_then(Value::as_str),
        Some("workspace_root must be an absolute path")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    assert!(message_payload
        .get("assistant_message")
        .and_then(|row| row.get("text"))
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("Clarification needed")));
}

#[tokio::test]
async fn workflow_plan_chat_message_returns_supported_edit_hint_for_unsupported_revision() {
    let state = test_state().await;
    let app = app_router(state);

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Send me a daily competitor digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace"
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Rewrite this as a four-stage market taxonomy with new custom step types."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("clarifier")
            .and_then(|row| row.get("field"))
            .and_then(Value::as_str),
        Some("general")
    );
    assert!(message_payload
        .get("clarifier")
        .and_then(|row| row.get("question"))
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("Supported edits in this slice")));
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_updates_title() {
    let state = test_state().await;
    let app = app_router(state);

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Send me a daily competitor digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace"
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Rename this plan to Weekly Competitor Digest and make it weekly."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("title"))
            .and_then(Value::as_str),
        Some("Weekly Competitor Digest")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec![
            "updated schedule".to_string(),
            "updated title".to_string()
        ])
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_adds_analysis_step_and_rewires_report() {
    let state = test_state().await;
    let app = app_router(state);

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Compare two competitor summaries and generate a report",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace"
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Add analysis before reporting."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    let steps = message_payload
        .get("plan")
        .and_then(|row| row.get("steps"))
        .and_then(Value::as_array)
        .cloned()
        .expect("steps");
    assert!(steps
        .iter()
        .any(|row| { row.get("step_id").and_then(Value::as_str) == Some("analyze_findings") }));
    let report_step = steps
        .iter()
        .find(|row| row.get("step_id").and_then(Value::as_str) == Some("generate_report"))
        .expect("report step");
    assert_eq!(
        report_step
            .get("depends_on")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("analyze_findings")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["added analysis step".to_string()])
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_adds_notification_step() {
    let state = test_state().await;
    let app = app_router(state);

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Research the market and generate a report",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace"
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Add a notification step and notify me when it's done."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    let steps = message_payload
        .get("plan")
        .and_then(|row| row.get("steps"))
        .and_then(Value::as_array)
        .cloned()
        .expect("steps");
    let notify_step = steps
        .iter()
        .find(|row| row.get("step_id").and_then(Value::as_str) == Some("notify_user"))
        .expect("notify step");
    assert_eq!(
        notify_step
            .get("depends_on")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("generate_report")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["added notification step".to_string()])
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_updates_execution_mode_preferences() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Monitor GitHub issues and write a daily digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace",
                "operator_preferences": {
                    "execution_mode": "team",
                    "max_parallel_agents": 1
                }
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Use swarm mode with 6 agents."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("operator_preferences"))
            .and_then(|row| row.get("execution_mode"))
            .and_then(Value::as_str),
        Some("swarm")
    );
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("operator_preferences"))
            .and_then(|row| row.get("max_parallel_agents"))
            .and_then(Value::as_u64),
        Some(6)
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec![
            "updated execution mode".to_string(),
            "updated max parallel agents".to_string()
        ])
    );

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert_eq!(stored.execution.max_parallel_agents, Some(6));
}

#[tokio::test]
async fn workflow_plan_chat_message_updates_model_override_preferences() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Monitor GitHub issues and write a daily digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace",
                "operator_preferences": {
                    "execution_mode": "team",
                    "max_parallel_agents": 1
                }
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Use openai model gpt-5.1 for this workflow."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("operator_preferences"))
            .and_then(|row| row.get("model_provider"))
            .and_then(Value::as_str),
        Some("openai")
    );
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("operator_preferences"))
            .and_then(|row| row.get("model_id"))
            .and_then(Value::as_str),
        Some("gpt-5.1")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["updated model override".to_string()])
    );

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert!(stored.agents.iter().all(|agent| {
        agent
            .model_policy
            .as_ref()
            .and_then(|policy| policy.get("default_model"))
            .and_then(|row| row.get("provider_id"))
            .and_then(Value::as_str)
            == Some("openai")
            && agent
                .model_policy
                .as_ref()
                .and_then(|policy| policy.get("default_model"))
                .and_then(|row| row.get("model_id"))
                .and_then(Value::as_str)
                == Some("gpt-5.1")
    }));
}

#[tokio::test]
async fn workflow_plan_chat_message_can_clear_model_override_preferences() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Monitor GitHub issues and write a daily digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace",
                "operator_preferences": {
                    "execution_mode": "team",
                    "max_parallel_agents": 1,
                    "model_provider": "openai",
                    "model_id": "gpt-5.1",
                    "role_models": {
                        "planner": {
                            "provider_id": "anthropic",
                            "model_id": "claude-sonnet-4"
                        }
                    }
                }
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Use the default model and clear model overrides."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert!(message_payload
        .get("plan")
        .and_then(|row| row.get("operator_preferences"))
        .and_then(|row| row.get("model_provider"))
        .is_none());
    assert!(message_payload
        .get("plan")
        .and_then(|row| row.get("operator_preferences"))
        .and_then(|row| row.get("model_id"))
        .is_none());
    assert!(message_payload
        .get("plan")
        .and_then(|row| row.get("operator_preferences"))
        .and_then(|row| row.get("role_models"))
        .is_none());
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["updated model override".to_string()])
    );

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert!(stored
        .agents
        .iter()
        .all(|agent| agent.model_policy.is_none()));
}

#[tokio::test]
async fn workflow_plan_chat_message_can_clear_allowed_mcp_servers() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Monitor GitHub issues and write a daily digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace",
                "allowed_mcp_servers": ["github", "slack"]
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Disable MCP and remove all MCP servers."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("allowed_mcp_servers"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["updated allowed MCP servers".to_string()])
    );

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert!(stored
        .agents
        .iter()
        .all(|agent| agent.mcp_policy.allowed_servers.is_empty()));
}

#[tokio::test]
async fn workflow_plan_chat_message_respects_only_mcp_server_constraints() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Monitor GitHub issues and write a daily digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace",
                "allowed_mcp_servers": ["github", "slack"]
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Use github only."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("allowed_mcp_servers"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["github".to_string()])
    );

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert!(stored
        .agents
        .iter()
        .all(|agent| agent.mcp_policy.allowed_servers == vec!["github".to_string()]));
}

#[tokio::test]
async fn workflow_plan_chat_message_can_switch_schedule_to_manual() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "prompt": "Send me a daily competitor digest",
                "plan_source": "automations_page",
                "workspace_root": "/tmp/initial-workspace"
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id")
        .to_string();

    let message_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": "Make this manual only and do not schedule it."
            })
            .to_string(),
        ))
        .expect("message request");
    let message_resp = app
        .clone()
        .oneshot(message_req)
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("schedule"))
            .and_then(|row| row.get("type"))
            .and_then(Value::as_str),
        Some("manual")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            }),
        Some(vec!["updated schedule".to_string()])
    );

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert_eq!(
        stored.schedule.schedule_type,
        crate::AutomationV2ScheduleType::Manual
    );
}
