use super::*;

#[tokio::test]
async fn setup_understand_intercepts_provider_setup() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "control_panel_chat",
                "text": "Use OpenRouter with Claude Sonnet"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("intercept")
    );
    assert_eq!(
        payload.get("intent_kind").and_then(Value::as_str),
        Some("provider_setup")
    );
}

#[tokio::test]
async fn setup_understand_passes_through_provider_comparison() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "channel",
                "text": "How does Tandem compare with Google Opal?"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("pass_through")
    );
}

#[tokio::test]
async fn setup_understand_intercepts_integration_setup() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "channel",
                "text": "Connect Notion so Tandem can read my docs"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("intercept")
    );
    assert_eq!(
        payload.get("intent_kind").and_then(Value::as_str),
        Some("integration_setup")
    );
}

#[tokio::test]
async fn setup_understand_intercepts_automation_creation() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "desktop_chat",
                "text": "Monitor GitHub issues and post a daily digest to Slack"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("intercept")
    );
    assert_eq!(
        payload.get("intent_kind").and_then(Value::as_str),
        Some("automation_create")
    );
    assert_eq!(
        payload
            .get("proposed_action")
            .and_then(|row| row.get("type"))
            .and_then(Value::as_str),
        Some("workflow_plan_preview")
    );
}

#[tokio::test]
async fn setup_understand_routes_channel_automation_to_automation_create() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "channel",
                "channel": "discord",
                "text": "Create an automation from this chat that posts a daily summary here",
                "trigger": {
                    "source": "mention",
                    "is_direct_message": false,
                    "was_explicitly_mentioned": true,
                    "is_reply_to_bot": false
                },
                "scope": {
                    "kind": "room",
                    "id": "room-1"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("intercept")
    );
    assert_eq!(
        payload.get("intent_kind").and_then(Value::as_str),
        Some("automation_create")
    );
}

#[tokio::test]
async fn setup_understand_intercepts_workflow_planning() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "control_panel_chat",
                "text": "Draft a workflow plan for coordinating releases across Slack and GitHub"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("intercept")
    );
    assert_eq!(
        payload.get("intent_kind").and_then(Value::as_str),
        Some("workflow_planner_create")
    );
    assert_eq!(
        payload
            .get("proposed_action")
            .and_then(|row| row.get("type"))
            .and_then(Value::as_str),
        Some("workflow_plan_preview")
    );
}

#[tokio::test]
async fn setup_understand_clarifies_missing_workflow_details() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "control_panel_chat",
                "text": "Create a workflow"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("clarify")
    );
    assert_eq!(
        payload.get("intent_kind").and_then(Value::as_str),
        Some("workflow_planner_create")
    );
    let question = payload
        .get("clarifier")
        .and_then(|row| row.get("question"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        question.contains("trigger or schedule")
            || question.contains("What should the workflow do?")
    );
    assert!(payload
        .get("clarifier")
        .and_then(|row| row.get("options"))
        .and_then(Value::as_array)
        .is_some_and(|options| !options.is_empty()));
    assert_eq!(
        payload
            .get("proposed_action")
            .and_then(|row| row.get("type"))
            .and_then(Value::as_str),
        Some("workflow_plan_preview")
    );
    let proposed_payload = payload
        .get("proposed_action")
        .and_then(|row| row.get("payload"))
        .expect("proposed action payload");
    assert_eq!(
        proposed_payload.get("prompt").and_then(Value::as_str),
        Some("create a workflow")
    );
    assert_eq!(
        proposed_payload.get("plan_source").and_then(Value::as_str),
        Some("control_panel_chat")
    );
}

#[tokio::test]
async fn setup_understand_clarifies_broad_setup_requests() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "desktop_chat",
                "text": "Set Tandem up for my workflow"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("clarify")
    );
    assert!(payload
        .get("clarifier")
        .and_then(|row| row.get("options"))
        .and_then(Value::as_array)
        .is_some_and(|options| !options.is_empty()));
}

#[tokio::test]
async fn setup_understand_passes_through_normal_chat() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "desktop_chat",
                "text": "Summarize this file"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("pass_through")
    );
}

#[tokio::test]
async fn setup_understand_passes_through_plain_github_url() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "control_panel_chat",
                "text": "https://github.com/frumu-ai/tandem/issues/42"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("pass_through")
    );
}

#[tokio::test]
async fn setup_understand_passes_through_read_this_github_url() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/setup/understand")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "surface": "control_panel_chat",
                "text": "read this https://github.com/frumu-ai/tandem/pull/123"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("pass_through")
    );
}
