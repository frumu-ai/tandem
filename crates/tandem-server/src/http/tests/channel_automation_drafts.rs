use super::*;

fn assert_no_channel_draft_confirmation_text(payload: &Value) {
    let rendered = serde_json::to_string(payload).expect("payload json");
    assert!(!rendered.contains("Reply `confirm` to create it"));
    assert!(!rendered.contains("Reply confirm to create it"));
    assert!(!rendered.contains("report to `same_chat`"));
    assert!(!rendered.contains("report to same_chat"));
    assert!(!rendered.contains("event-driven"));
}

#[tokio::test]
async fn channel_automation_draft_collects_previews_and_confirms() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_req = Request::builder()
        .method("POST")
        .uri("/automations/channel-drafts")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "text": "Create an automation",
                "session_id": "session-channel-1",
                "thread_key": "discord:room-1",
                "channel_context": {
                    "source_platform": "discord",
                    "scope_kind": "room",
                    "scope_id": "room-1",
                    "reply_target": "room-1",
                    "sender": "alice"
                },
                "allowed_tools": ["websearch", "memory_store"],
                "allowed_mcp_servers": ["github"]
            })
            .to_string(),
        ))
        .expect("request");
    let start_resp = app.clone().oneshot(start_req).await.expect("response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let start_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        start_payload
            .get("draft")
            .and_then(|draft| draft.get("status"))
            .and_then(Value::as_str),
        Some("collecting")
    );
    assert_eq!(
        start_payload
            .get("draft")
            .and_then(|draft| draft.get("question"))
            .and_then(|question| question.get("field"))
            .and_then(Value::as_str),
        Some("goal")
    );
    let draft_id = start_payload
        .get("draft")
        .and_then(|draft| draft.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id")
        .to_string();

    let goal_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/channel-drafts/{draft_id}/answer"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "answer": "Post a support summary here" }).to_string(),
        ))
        .expect("request");
    let goal_resp = app.clone().oneshot(goal_req).await.expect("response");
    assert_eq!(goal_resp.status(), StatusCode::OK);
    let body = to_bytes(goal_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let goal_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        goal_payload
            .get("draft")
            .and_then(|draft| draft.get("question"))
            .and_then(|question| question.get("field"))
            .and_then(Value::as_str),
        Some("schedule_hint")
    );

    let schedule_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/channel-drafts/{draft_id}/answer"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "answer": "daily at 9am" }).to_string()))
        .expect("request");
    let schedule_resp = app.clone().oneshot(schedule_req).await.expect("response");
    assert_eq!(schedule_resp.status(), StatusCode::OK);
    let body = to_bytes(schedule_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let preview_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        preview_payload
            .get("draft")
            .and_then(|draft| draft.get("status"))
            .and_then(Value::as_str),
        Some("preview_ready")
    );
    assert!(preview_payload
        .get("message")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("Reply `confirm`")));

    let pending_req = Request::builder()
        .method("GET")
        .uri("/automations/channel-drafts/pending?channel=discord&scope_id=room-1&sender=alice")
        .body(Body::empty())
        .expect("request");
    let pending_resp = app.clone().oneshot(pending_req).await.expect("response");
    assert_eq!(pending_resp.status(), StatusCode::OK);
    let body = to_bytes(pending_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let pending_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        pending_payload.get("count").and_then(Value::as_u64),
        Some(1)
    );

    let confirm_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/channel-drafts/{draft_id}/confirm"))
        .body(Body::empty())
        .expect("request");
    let confirm_resp = app.clone().oneshot(confirm_req).await.expect("response");
    assert_eq!(confirm_resp.status(), StatusCode::OK);
    let body = to_bytes(confirm_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let confirm_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        confirm_payload
            .get("draft")
            .and_then(|draft| draft.get("status"))
            .and_then(Value::as_str),
        Some("applied")
    );
    let automation = confirm_payload.get("automation").expect("automation");
    assert_eq!(
        automation.get("status").and_then(Value::as_str),
        Some("active")
    );
    assert_eq!(
        automation
            .get("schedule")
            .and_then(|schedule| schedule.get("type"))
            .and_then(Value::as_str),
        Some("cron")
    );
    assert_eq!(
        automation
            .get("schedule")
            .and_then(|schedule| schedule.get("cron_expression"))
            .and_then(Value::as_str),
        Some("0 0 9 * * * *")
    );
    assert_eq!(
        automation
            .get("metadata")
            .and_then(|metadata| metadata.get("created_from"))
            .and_then(Value::as_str),
        Some("channel_automation_draft")
    );
    assert_eq!(
        automation
            .get("metadata")
            .and_then(|metadata| metadata.get("channel_context"))
            .and_then(|context| context.get("source_platform"))
            .and_then(Value::as_str),
        Some("discord")
    );
    assert_eq!(
        automation
            .get("agents")
            .and_then(Value::as_array)
            .and_then(|agents| agents.first())
            .and_then(|agent| agent.get("mcp_policy"))
            .and_then(|policy| policy.get("allowed_servers"))
            .and_then(Value::as_array)
            .map(|servers| servers.contains(&Value::String("github".to_string()))),
        Some(true)
    );
}

#[tokio::test]
async fn channel_automation_draft_start_fails_closed_when_workflow_drafts_disabled() {
    let state = test_state().await;
    let app = app_router(state);

    let start_req = Request::builder()
        .method("POST")
        .uri("/automations/channel-drafts")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "text": "What time does sponsor booth setup start, and when must it be finished?",
                "session_id": "session-channel-disabled",
                "thread_key": "telegram:topic-disabled",
                "workflow_planner_enabled": false,
                "strict_kb_grounding": false,
                "factual_question": true,
                "explicit_workflow_intent": false,
                "channel_context": {
                    "source_platform": "telegram",
                    "scope_kind": "topic",
                    "scope_id": "topic-disabled",
                    "reply_target": "topic-disabled",
                    "sender": "alice"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let start_resp = app.clone().oneshot(start_req).await.expect("response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_no_channel_draft_confirmation_text(&payload);
    assert_eq!(payload.get("blocked").and_then(Value::as_bool), Some(true));
    assert_eq!(
        payload.get("block_reason").and_then(Value::as_str),
        Some("workflow_drafting_disabled")
    );
    assert_eq!(
        payload
            .get("draft")
            .and_then(|draft| draft.get("status"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
}

#[tokio::test]
async fn channel_automation_draft_answer_fails_closed_for_strict_kb_factual_question() {
    let state = test_state().await;
    let app = app_router(state);

    let start_req = Request::builder()
        .method("POST")
        .uri("/automations/channel-drafts")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "text": "Create a workflow that sends a sponsor setup reminder every event morning.",
                "session_id": "session-channel-strict",
                "thread_key": "telegram:topic-strict",
                "workflow_planner_enabled": true,
                "strict_kb_grounding": true,
                "factual_question": false,
                "explicit_workflow_intent": true,
                "channel_context": {
                    "source_platform": "telegram",
                    "scope_kind": "topic",
                    "scope_id": "topic-strict",
                    "reply_target": "topic-strict",
                    "sender": "alice"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let start_resp = app.clone().oneshot(start_req).await.expect("response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let start_payload: Value = serde_json::from_slice(&body).expect("json");
    let draft_id = start_payload
        .get("draft")
        .and_then(|draft| draft.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id")
        .to_string();

    let answer_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/channel-drafts/{draft_id}/answer"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "answer": "What time does sponsor booth setup start, and when must it be finished?",
                "workflow_planner_enabled": true,
                "strict_kb_grounding": true,
                "factual_question": true,
                "explicit_workflow_intent": false
            })
            .to_string(),
        ))
        .expect("request");
    let answer_resp = app.clone().oneshot(answer_req).await.expect("response");
    assert_eq!(answer_resp.status(), StatusCode::OK);
    let body = to_bytes(answer_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let answer_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_no_channel_draft_confirmation_text(&answer_payload);
    assert_eq!(
        answer_payload.get("block_reason").and_then(Value::as_str),
        Some("strict_kb_factual_question")
    );
    assert_eq!(
        answer_payload
            .get("draft")
            .and_then(|draft| draft.get("status"))
            .and_then(Value::as_str),
        Some("cancelled")
    );

    let pending_req = Request::builder()
        .method("GET")
        .uri("/automations/channel-drafts/pending?channel=telegram&scope_id=topic-strict&sender=alice")
        .body(Body::empty())
        .expect("request");
    let pending_resp = app.clone().oneshot(pending_req).await.expect("response");
    assert_eq!(pending_resp.status(), StatusCode::OK);
    let body = to_bytes(pending_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let pending_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        pending_payload.get("count").and_then(Value::as_u64),
        Some(0)
    );
}

#[tokio::test]
async fn channel_automation_draft_can_cancel_preview() {
    let state = test_state().await;
    let app = app_router(state);

    let start_req = Request::builder()
        .method("POST")
        .uri("/automations/channel-drafts")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "text": "Create an automation that posts a daily summary here every day",
                "channel_context": {
                    "source_platform": "telegram",
                    "scope_kind": "topic",
                    "scope_id": "topic-1",
                    "reply_target": "topic-1",
                    "sender": "bob"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let start_resp = app.clone().oneshot(start_req).await.expect("response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let start_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        start_payload
            .get("draft")
            .and_then(|draft| draft.get("status"))
            .and_then(Value::as_str),
        Some("preview_ready")
    );
    let draft_id = start_payload
        .get("draft")
        .and_then(|draft| draft.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id");

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/channel-drafts/{draft_id}/cancel"))
        .body(Body::empty())
        .expect("request");
    let cancel_resp = app.clone().oneshot(cancel_req).await.expect("response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
    let body = to_bytes(cancel_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let cancel_payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        cancel_payload
            .get("draft")
            .and_then(|draft| draft.get("status"))
            .and_then(Value::as_str),
        Some("cancelled")
    );

    let confirm_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/channel-drafts/{draft_id}/confirm"))
        .body(Body::empty())
        .expect("request");
    let confirm_resp = app.clone().oneshot(confirm_req).await.expect("response");
    assert_eq!(confirm_resp.status(), StatusCode::CONFLICT);
}
