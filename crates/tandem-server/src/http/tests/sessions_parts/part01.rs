use super::*;
use async_trait::async_trait;
use futures::stream;
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use tandem_providers::{ChatMessage, Provider, StreamChunk};
use tandem_types::{ModelInfo, ModelSpec, ProviderInfo, ToolMode, ToolSchema};
use tokio_util::sync::CancellationToken;

struct StreamedWriteTestProvider;

#[async_trait]
impl Provider for StreamedWriteTestProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "streamed-test".to_string(),
            name: "Streamed Test".to_string(),
            models: vec![ModelInfo {
                id: "streamed-test-1".to_string(),
                provider_id: "streamed-test".to_string(),
                display_name: "Streamed Test 1".to_string(),
                context_window: 8192,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok(String::new())
    }

    async fn stream(
        &self,
        _messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let chunks = vec![
            Ok(StreamChunk::ToolCallStart {
                id: "call_stream_1".to_string(),
                name: "write".to_string(),
            }),
            Ok(StreamChunk::ToolCallDelta {
                id: "call_stream_1".to_string(),
                args_delta: r#"{"path":"game.html","content":"<html>"#.to_string(),
            }),
            Ok(StreamChunk::ToolCallDelta {
                id: "call_stream_1".to_string(),
                args_delta: r#"draft</html>"}"#.to_string(),
            }),
            Ok(StreamChunk::ToolCallEnd {
                id: "call_stream_1".to_string(),
            }),
            Ok(StreamChunk::Done {
                finish_reason: "tool_calls".to_string(),
                usage: None,
            }),
        ];
        Ok(Box::pin(stream::iter(chunks)))
    }
}

#[tokio::test]
async fn session_todo_route_returns_normalized_items() {
    let state = test_state().await;
    let session = Session::new(Some("test".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    state
        .storage
        .set_todos(
            &session_id,
            vec![
                json!({"content":"one"}),
                json!({"text":"two","status":"in_progress"}),
            ],
        )
        .await
        .expect("set todos");

    let app = app_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}/todo"))
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let todos = payload.as_array().expect("todos array");
    assert_eq!(todos.len(), 2);
    for todo in todos {
        assert!(todo.get("id").and_then(|v| v.as_str()).is_some());
        assert!(todo.get("content").and_then(|v| v.as_str()).is_some());
        assert!(todo.get("status").and_then(|v| v.as_str()).is_some());
    }
}

#[tokio::test]
async fn update_session_refreshes_mcp_permissions() {
    let state = test_state().await;
    let session = Session::new(Some("perm refresh".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");

    let app = app_router(state.clone());
    let req = Request::builder()
        .method("PATCH")
        .uri(format!("/session/{session_id}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "permission": [
                    {"permission": "mcp*", "pattern": "*", "action": "allow"}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let action = state.permissions.evaluate("mcp_list", "mcp_list").await;
    assert!(matches!(action, tandem_core::PermissionAction::Allow));
}

#[tokio::test]
async fn session_part_persister_stores_tool_parts_in_session_history() {
    let state = test_state().await;
    let task = tokio::spawn(crate::run_session_part_persister(state.clone()));
    let session = Session::new(
        Some("persist tool parts".to_string()),
        Some(".".to_string()),
    );
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let message = Message::new(
        MessageRole::User,
        vec![MessagePart::Text {
            text: "build ui".to_string(),
        }],
    );
    let message_id = message.id.clone();
    state
        .storage
        .append_message(&session_id, message)
        .await
        .expect("append");

    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "sessionID": session_id,
            "part": {
                "type": "tool",
                "messageID": message_id,
                "tool": "write",
                "args": { "path": "game.html", "content": "<html></html>" },
                "state": "running"
            }
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "sessionID": session_id,
            "part": {
                "type": "tool",
                "messageID": message_id,
                "tool": "write",
                "result": "ok",
                "state": "completed"
            }
        }),
    ));

    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let session = state
                .storage
                .get_session(&session_id)
                .await
                .expect("session");
            let message = session
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .expect("message");
            if message.parts.len() > 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("tool part persisted");

    let session = state
        .storage
        .get_session(&session_id)
        .await
        .expect("session");
    let message = session
        .messages
        .iter()
        .find(|message| message.id == message_id)
        .expect("message");
    match &message.parts[1] {
        MessagePart::ToolInvocation { tool, result, .. } => {
            assert_eq!(tool, "write");
            assert_eq!(result.as_ref(), Some(&json!("ok")));
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }

    task.abort();
}

#[tokio::test]
async fn session_part_persister_stores_runtime_wire_tool_parts_in_session_history() {
    let state = test_state().await;
    let task = tokio::spawn(crate::run_session_part_persister(state.clone()));
    let session = Session::new(
        Some("persist runtime wire tool parts".to_string()),
        Some(".".to_string()),
    );
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let message = Message::new(
        MessageRole::User,
        vec![MessagePart::Text {
            text: "inspect workspace".to_string(),
        }],
    );
    let message_id = message.id.clone();
    state
        .storage
        .append_message(&session_id, message)
        .await
        .expect("append");

    let invoke = tandem_wire::WireMessagePart::tool_invocation(
        &session_id,
        &message_id,
        "glob",
        json!({ "pattern": "*" }),
    );
    let result = tandem_wire::WireMessagePart::tool_result(
        &session_id,
        &message_id,
        "glob",
        Some(json!({ "pattern": "*" })),
        json!(["README.md"]),
    );

    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "part": invoke
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "part": result
        }),
    ));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let session = state
                .storage
                .get_session(&session_id)
                .await
                .expect("session");
            let message = session
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .expect("message");
            if message.parts.len() > 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("runtime tool part persisted");

    let session = state
        .storage
        .get_session(&session_id)
        .await
        .expect("session");
    let message = session
        .messages
        .iter()
        .find(|message| message.id == message_id)
        .expect("message");
    match &message.parts[1] {
        MessagePart::ToolInvocation { tool, result, .. } => {
            assert_eq!(tool, "glob");
            assert_eq!(result.as_ref(), Some(&json!(["README.md"])));
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }

    task.abort();
}

#[tokio::test]
async fn session_part_persister_stores_result_args_without_prior_invoke() {
    let state = test_state().await;
    let task = tokio::spawn(crate::run_session_part_persister(state.clone()));
    let session = Session::new(
        Some("persist result args".to_string()),
        Some(".".to_string()),
    );
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let message = Message::new(
        MessageRole::User,
        vec![MessagePart::Text {
            text: "build ui".to_string(),
        }],
    );
    let message_id = message.id.clone();
    state
        .storage
        .append_message(&session_id, message)
        .await
        .expect("append");

    let result = tandem_wire::WireMessagePart::tool_result(
        &session_id,
        &message_id,
        "write",
        Some(json!({ "path": "game.html", "content": "<html></html>" })),
        json!(null),
    );

    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "part": result
        }),
    ));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let session = state
                .storage
                .get_session(&session_id)
                .await
                .expect("session");
            let message = session
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .expect("message");
            if message.parts.len() > 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("result tool part persisted");

    let session = state
        .storage
        .get_session(&session_id)
        .await
        .expect("session");
    let message = session
        .messages
        .iter()
        .find(|message| message.id == message_id)
        .expect("message");
    match &message.parts[1] {
        MessagePart::ToolInvocation {
            tool, args, result, ..
        } => {
            assert_eq!(tool, "write");
            assert_eq!(args["path"], "game.html");
            assert_eq!(args["content"], "<html></html>");
            assert_eq!(result.as_ref(), None);
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }

    task.abort();
}

#[tokio::test]
async fn session_part_persister_preserves_streamed_preview_args_across_failed_write_result() {
    let state = test_state().await;
    let task = tokio::spawn(crate::run_session_part_persister(state.clone()));
    let session = Session::new(
        Some("persist streamed preview write args".to_string()),
        Some(".".to_string()),
    );
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let message = Message::new(
        MessageRole::User,
        vec![MessagePart::Text {
            text: "build game".to_string(),
        }],
    );
    let message_id = message.id.clone();
    state
        .storage
        .append_message(&session_id, message)
        .await
        .expect("append");

    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "sessionID": session_id,
            "part": {
                "type": "tool",
                "messageID": message_id,
                "tool": "write",
                "args": {},
                "state": "running"
            },
            "toolCallDelta": {
                "id": "call_123",
                "tool": "write",
                "parsedArgsPreview": {
                    "path": "game.html",
                    "content": "<html>draft</html>"
                }
            }
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "sessionID": session_id,
            "part": {
                "type": "tool",
                "messageID": message_id,
                "tool": "write",
                "args": {},
                "state": "failed",
                "error": "WRITE_ARGS_EMPTY_FROM_PROVIDER"
            }
        }),
    ));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let session = state
                .storage
                .get_session(&session_id)
                .await
                .expect("session");
            let message = session
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .expect("message");
            if message.parts.len() > 1 {
                match &message.parts[1] {
                    MessagePart::ToolInvocation { args, error, .. }
                        if args.get("path").and_then(|value| value.as_str())
                            == Some("game.html")
                            && error.as_deref() == Some("WRITE_ARGS_EMPTY_FROM_PROVIDER") =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("tool preview + failure persisted");

    let session = state
        .storage
        .get_session(&session_id)
        .await
        .expect("session");
    let message = session
        .messages
        .iter()
        .find(|message| message.id == message_id)
        .expect("message");
    match &message.parts[1] {
        MessagePart::ToolInvocation {
            tool, args, error, ..
        } => {
            assert_eq!(tool, "write");
            assert_eq!(args["path"], "game.html");
            assert_eq!(args["content"], "<html>draft</html>");
            assert_eq!(error.as_deref(), Some("WRITE_ARGS_EMPTY_FROM_PROVIDER"));
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }

    task.abort();
}

#[tokio::test]
async fn session_part_persister_falls_back_to_streamed_raw_args_preview_when_parse_preview_missing()
{
    let state = test_state().await;
    let task = tokio::spawn(crate::run_session_part_persister(state.clone()));
    let session = Session::new(
        Some("persist streamed raw write args".to_string()),
        Some(".".to_string()),
    );
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let message = Message::new(
        MessageRole::User,
        vec![MessagePart::Text {
            text: "build game".to_string(),
        }],
    );
    let message_id = message.id.clone();
    state
        .storage
        .append_message(&session_id, message)
        .await
        .expect("append");

    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "sessionID": session_id,
            "part": {
                "type": "tool",
                "messageID": message_id,
                "tool": "write",
                "args": {},
                "state": "running"
            },
            "toolCallDelta": {
                "id": "call_raw_only",
                "tool": "write",
                "rawArgsPreview": "{\"path\":\"game.html\",\"content\":\"<html>draft</html>\"}"
            }
        }),
    ));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let session = state
                .storage
                .get_session(&session_id)
                .await
                .expect("session");
            let message = session
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .expect("message");
            if message.parts.len() > 1 {
                match &message.parts[1] {
                    MessagePart::ToolInvocation { args, .. }
                        if args.as_str()
                            == Some(
                                "{\"path\":\"game.html\",\"content\":\"<html>draft</html>\"}",
                            ) =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("tool raw preview persisted");

    let session = state
        .storage
        .get_session(&session_id)
        .await
        .expect("session");
    let message = session
        .messages
        .iter()
        .find(|message| message.id == message_id)
        .expect("message");
    match &message.parts[1] {
        MessagePart::ToolInvocation { tool, args, .. } => {
            assert_eq!(tool, "write");
            assert_eq!(
                args.as_str(),
                Some("{\"path\":\"game.html\",\"content\":\"<html>draft</html>\"}")
            );
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }

    task.abort();
}

#[tokio::test]
async fn answer_question_alias_route_returns_ok() {
    let state = test_state().await;
    let session = Session::new(Some("q".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let question = state
        .storage
        .add_question_request(
            &session_id,
            "m1",
            vec![json!({"header":"h","question":"q","options":[]})],
        )
        .await
        .expect("question");

    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/sessions/{}/questions/{}/answer",
            session_id, question.id
        ))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"answer":"ok"}"#))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
}

#[tokio::test]
async fn api_session_alias_lists_sessions() {
    let state = test_state().await;
    let session = Session::new(Some("alias".to_string()), Some(".".to_string()));
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/api/session")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload.as_array().map(|v| !v.is_empty()).unwrap_or(false));
}

#[tokio::test]
async fn create_session_accepts_camel_case_model_spec() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "camel-model",
                "model": {
                    "providerID": "openrouter",
                    "modelID": "openai/gpt-4o-mini"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let model = payload.get("model").cloned().unwrap_or_else(|| json!({}));
    assert_eq!(
        model.get("providerID").and_then(|v| v.as_str()),
        Some("openrouter")
    );
    assert_eq!(
        model.get("modelID").and_then(|v| v.as_str()),
        Some("openai/gpt-4o-mini")
    );
    assert!(payload.get("environment").is_some());
    assert!(payload.get("projectID").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn create_session_binds_workspace_project_id() {
    let state = test_state().await;
    let workspace_root = std::env::temp_dir()
        .join(format!("tandem-http-create-session-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "workspace-bound",
                "workspace_root": workspace_root,
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("projectID").and_then(|v| v.as_str()),
        tandem_core::workspace_project_id(
            payload
                .get("workspaceRoot")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
        )
        .as_deref()
    );
}

#[tokio::test]
async fn create_session_uses_request_tenant_context_and_emits_tenant_scoped_event() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "north")
        .header("x-user-id", "user-1")
        .body(Body::from(
            json!({
                "title": "tenant-bound",
                "directory": "."
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let session_id = payload
        .get("id")
        .and_then(|value| value.as_str())
        .expect("session id");

    let stored_session = state
        .storage
        .get_session(session_id)
        .await
        .expect("session");
    assert_eq!(stored_session.tenant_context.org_id, "acme");
    assert_eq!(stored_session.tenant_context.workspace_id, "north");
    assert_eq!(
        stored_session.tenant_context.actor_id.as_deref(),
        Some("user-1")
    );

    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "session.created" {
                return event;
            }
        }
    })
    .await
    .expect("session.created timeout");
    assert_eq!(
        event
            .properties
            .get("tenantContext")
            .and_then(|value| value.get("org_id"))
            .and_then(Value::as_str),
        Some("acme")
    );
    assert_eq!(
        event
            .properties
            .get("tenantContext")
            .and_then(|value| value.get("workspace_id"))
            .and_then(Value::as_str),
        Some("north")
    );
    assert_eq!(
        event
            .properties
            .get("tenantContext")
            .and_then(|value| value.get("actor_id"))
            .and_then(Value::as_str),
        Some("user-1")
    );
}

#[tokio::test]
async fn post_session_message_returns_wire_message() {
    let state = test_state().await;
    let session = Session::new(Some("post-msg".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"parts":[{"type":"text","text":"hello from test"}]}).to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload.get("info").is_some());
    assert!(payload.get("parts").is_some());
}

#[tokio::test]
async fn session_listing_honors_workspace_scope_query() {
    let state = test_state().await;
    let ws_a = std::env::temp_dir()
        .join(format!("tandem-http-ws-a-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let ws_b = std::env::temp_dir()
        .join(format!("tandem-http-ws-b-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();

    let mut session_a = Session::new(Some("A".to_string()), Some(ws_a.clone()));
    session_a.workspace_root = Some(ws_a.clone());
    state.storage.save_session(session_a).await.expect("save A");

    let mut session_b = Session::new(Some("B".to_string()), Some(ws_b.clone()));
    session_b.workspace_root = Some(ws_b.clone());
    state.storage.save_session(session_b).await.expect("save B");

    let app = app_router(state);
    let encoded_ws_a = ws_a.replace('\\', "%5C").replace(':', "%3A");
    let scoped_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/session?scope=workspace&workspace={}",
            encoded_ws_a
        ))
        .body(Body::empty())
        .expect("request");
    let scoped_resp = app.clone().oneshot(scoped_req).await.expect("response");
    assert_eq!(scoped_resp.status(), StatusCode::OK);
    let scoped_body = to_bytes(scoped_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let scoped_payload: Value = serde_json::from_slice(&scoped_body).expect("json");
    assert_eq!(scoped_payload.as_array().map(|v| v.len()), Some(1));

    let global_req = Request::builder()
        .method("GET")
        .uri("/session?scope=global")
        .body(Body::empty())
        .expect("request");
    let global_resp = app.oneshot(global_req).await.expect("response");
    assert_eq!(global_resp.status(), StatusCode::OK);
    let global_body = to_bytes(global_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let global_payload: Value = serde_json::from_slice(&global_body).expect("json");
    assert_eq!(global_payload.as_array().map(|v| v.len()), Some(2));
}

#[tokio::test]
async fn attach_session_route_updates_workspace_metadata() {
    let state = test_state().await;
    let ws_a = std::env::temp_dir()
        .join(format!("tandem-http-attach-a-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let ws_b = std::env::temp_dir()
        .join(format!("tandem-http-attach-b-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let mut session = Session::new(Some("attach".to_string()), Some(ws_a.clone()));
    session.workspace_root = Some(ws_a);
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");

    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/attach"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"target_workspace": ws_b, "reason_tag": "manual_attach"}).to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("attachReason").and_then(|v| v.as_str()),
        Some("manual_attach")
    );
    assert!(payload
        .get("workspaceRoot")
        .and_then(|v| v.as_str())
        .is_some());
    assert_eq!(
        payload.get("projectID").and_then(|v| v.as_str()),
        tandem_core::workspace_project_id(
            payload
                .get("workspaceRoot")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
        )
        .as_deref()
    );
}

#[tokio::test]
async fn message_part_updated_event_contains_required_wire_fields() {
    let state = test_state().await;
    let session = Session::new(Some("sse-shape".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"parts":[{"type":"text","text":"hello streaming"}]}).to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "message.part.updated" {
                return event;
            }
        }
    })
    .await
    .expect("message.part.updated timeout");

    let part = event
        .properties
        .get("part")
        .cloned()
        .unwrap_or_else(|| json!({}));
    assert!(part.get("id").and_then(|v| v.as_str()).is_some());
    assert_eq!(
        part.get("sessionID").and_then(|v| v.as_str()),
        Some(session_id.as_str())
    );
    assert!(part.get("messageID").and_then(|v| v.as_str()).is_some());
    assert!(part.get("type").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn prompt_async_streamed_write_preserves_provider_call_id_and_args_lineage() {
    let state = test_state().await;
    state
        .providers
        .replace_for_test(
            vec![Arc::new(StreamedWriteTestProvider)],
            Some("streamed-test".to_string()),
        )
        .await;
    let task = tokio::spawn(crate::run_session_part_persister(state.clone()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut rx = state.event_bus.subscribe();
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-streamed-write-lineage-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut session = Session::new(
        Some("streamed write lineage".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.model = Some(ModelSpec {
        provider_id: "streamed-test".to_string(),
        model_id: "streamed-test-1".to_string(),
    });
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, vec!["write".to_string()])
        .await;
    state
        .engine_loop
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    state
        .engine_loop
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "create game.html now".to_string(),
                }],
                model: Some(ModelSpec {
                    provider_id: "streamed-test".to_string(),
                    model_id: "streamed-test-1".to_string(),
                }),
                agent: None,
                tool_mode: Some(ToolMode::Required),
                tool_allowlist: None,
                context_mode: None,
                write_required: None,
                prewrite_requirements: None,
            },
        )
        .await
        .expect("run prompt");

    let mut saw_delta_preview = false;
    let mut saw_pending_or_result_with_call_id = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !saw_delta_preview || !saw_pending_or_result_with_call_id {
            let event = rx.recv().await.expect("event");
            if event.event_type != "message.part.updated" {
                continue;
            }
            if !saw_delta_preview {
                if let Some(delta) = event.properties.get("toolCallDelta") {
                    if delta.get("id").and_then(|value| value.as_str()) == Some("call_stream_1")
                        && delta.get("tool").and_then(|value| value.as_str()) == Some("write")
                        && delta
                            .get("rawArgsPreview")
                            .and_then(|value| value.as_str())
                            .is_some_and(|value| value.contains("game.html"))
                        && delta
                            .get("parsedArgsPreview")
                            .and_then(|value| value.get("path"))
                            .and_then(|value| value.as_str())
                            == Some("game.html")
                    {
                        saw_delta_preview = true;
                    }
                }
            }
            if !saw_pending_or_result_with_call_id {
                if let Some(part) = event.properties.get("part") {
                    if part.get("id").and_then(|value| value.as_str()) == Some("call_stream_1")
                        && part.get("tool").and_then(|value| value.as_str()) == Some("write")
                        && part
                            .get("args")
                            .and_then(|value| value.get("path"))
                            .and_then(|value| value.as_str())
                            == Some("game.html")
                    {
                        saw_pending_or_result_with_call_id = true;
                    }
                }
            }
        }
    })
    .await
    .expect("streamed call id + args lineage events");

    let written = std::fs::read_to_string(workspace_root.join("game.html")).expect("written file");
    assert_eq!(written, "<html>draft</html>");

    state
        .engine_loop
        .clear_session_auto_approve_permissions(&session_id)
        .await;
    task.abort();
    let _ = std::fs::remove_dir_all(&workspace_root);
}

