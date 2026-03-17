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

#[test]
fn normalize_run_event_adds_required_fields() {
    let event = EngineEvent::new(
        "message.part.updated",
        json!({
            "part": { "type": "text" },
            "delta": "hello"
        }),
    );
    let normalized = normalize_run_event(event, "s-1", "r-1");
    assert_eq!(
        normalized
            .properties
            .get("sessionID")
            .and_then(|v| v.as_str()),
        Some("s-1")
    );
    assert_eq!(
        normalized.properties.get("runID").and_then(|v| v.as_str()),
        Some("r-1")
    );
    assert_eq!(
        normalized
            .properties
            .get("channel")
            .and_then(|v| v.as_str()),
        Some("assistant")
    );
}

#[test]
fn infer_event_channel_routes_tool_message_parts() {
    let channel = infer_event_channel(
        "message.part.updated",
        &serde_json::from_value::<serde_json::Map<String, Value>>(json!({
            "part": { "type": "tool-result" }
        }))
        .expect("map"),
    );
    assert_eq!(channel, "tool");
}

#[test]
fn extract_persistable_tool_part_uses_streamed_args_preview_when_part_args_empty() {
    let properties = json!({
        "part": {
            "type": "tool",
            "tool": "write",
            "messageID": "msg_123",
            "args": {}
        },
        "toolCallDelta": {
            "parsedArgsPreview": {
                "path": "game.html",
                "content": "<html></html>"
            }
        }
    });
    let (message_id, part) = crate::extract_persistable_tool_part(&properties).expect("tool part");
    assert_eq!(message_id, "msg_123");
    match part {
        tandem_types::MessagePart::ToolInvocation { tool, args, .. } => {
            assert_eq!(tool, "write");
            assert_eq!(args["path"], "game.html");
            assert_eq!(args["content"], "<html></html>");
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }
}

#[tokio::test]
async fn prompt_async_permission_approve_executes_tool_and_emits_todo_update() {
    let state = test_state().await;
    let session = Session::new(Some("perm".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let prompt_body = json!({
        "parts": [
            {
                "type": "text",
                "text": "/tool todo_write {\"todos\":[{\"content\":\"write tests\"}]}"
            }
        ]
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async"))
        .header("content-type", "application/json")
        .body(Body::from(prompt_body.to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let request_id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "permission.asked" {
                let id = event
                    .properties
                    .get("requestID")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !id.is_empty() {
                    return id;
                }
            }
        }
    })
    .await
    .expect("permission asked timeout");

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/sessions/{}/tools/{}/approve",
            session_id, request_id
        ))
        .body(Body::empty())
        .expect("approve request");
    let approve_resp = app.clone().oneshot(approve_req).await.expect("approve");
    assert_eq!(approve_resp.status(), StatusCode::OK);

    let todo_event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "todo.updated" {
                return event;
            }
        }
    })
    .await
    .expect("todo.updated timeout");

    assert_eq!(
        todo_event
            .properties
            .get("sessionID")
            .and_then(|v| v.as_str()),
        Some(session_id.as_str())
    );
    let todos = todo_event
        .properties
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(todos.len(), 1);
    assert_eq!(
        todos[0].get("content").and_then(|v| v.as_str()),
        Some("write tests")
    );
}

#[tokio::test]
async fn approve_route_returns_error_envelope_for_unknown_request() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/sessions/s1/tools/missing/approve")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("permission_request_not_found")
    );
    assert!(payload.get("error").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn prompt_async_return_run_returns_202_with_run_id_and_attach_stream() {
    let state = test_state().await;
    let session = Session::new(Some("return-run".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async?return=run"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"parts":[{"type":"text","text":"hello return=run"}]}).to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let run_id = payload.get("runID").and_then(|v| v.as_str()).unwrap_or("");
    let attach = payload
        .get("attachEventStream")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(!run_id.is_empty());
    assert_eq!(
        context_run_id,
        crate::http::session_context_run_id(&session_id)
    );
    assert_eq!(
        attach,
        format!("/event?sessionID={session_id}&runID={run_id}")
    );
    let context_run_resp = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{context_run_id}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(context_run_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_session_run_returns_active_metadata_while_run_is_in_flight() {
    let state = test_state().await;
    let session = Session::new(Some("active-run".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());

    let first_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async?return=run"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [
                        {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"hold run\"}]}"}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
    let first_resp = app.clone().oneshot(first_req).await.expect("response");
    assert_eq!(first_resp.status(), StatusCode::ACCEPTED);
    let first_body = to_bytes(first_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let first_payload: Value = serde_json::from_slice(&first_body).expect("json");
    let run_id = first_payload
        .get("runID")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(!run_id.is_empty());

    let run_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}/run"))
        .body(Body::empty())
        .expect("request");
    let run_resp = app.oneshot(run_req).await.expect("response");
    assert_eq!(run_resp.status(), StatusCode::OK);
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("json");
    let active = run_payload.get("active").cloned().unwrap_or(Value::Null);
    assert_eq!(
        active.get("runID").and_then(|v| v.as_str()),
        Some(run_id.as_str())
    );
    assert_eq!(
        run_payload
            .get("linked_context_run_id")
            .and_then(|v| v.as_str()),
        Some(crate::http::session_context_run_id(&session_id).as_str())
    );
    let context_run_resp = app_router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/runs/{}",
                    crate::http::session_context_run_id(&session_id)
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(context_run_resp.status(), StatusCode::OK);

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/cancel"))
        .body(Body::empty())
        .expect("cancel request");
    let cancel_resp = app_router(state)
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn session_context_run_journaler_persists_session_run_lineage() {
    let state = test_state().await;
    let task = tokio::spawn(crate::run_session_context_run_journaler(state.clone()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut session = Session::new(
        Some("journal interactive run".to_string()),
        Some(".".to_string()),
    );
    session.workspace_root = Some("/tmp/tandem-session-journal".to_string());
    session.project_id = Some("proj-session-journal".to_string());
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");

    state.event_bus.publish(EngineEvent::new(
        "session.run.started",
        json!({
            "sessionID": session_id,
            "runID": "run-session-journal-1",
            "agentID": "interactive",
            "agentProfile": "interactive",
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "sessionID": session_id,
            "runID": "run-session-journal-1",
            "part": {
                "type": "tool",
                "tool": "read",
                "state": "running",
                "args": { "path": "README.md" }
            }
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "session.run.finished",
        json!({
            "sessionID": session_id,
            "runID": "run-session-journal-1",
            "status": "completed",
        }),
    ));

    let context_run_id = crate::http::session_context_run_id(&session_id);
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let run = crate::http::context_runs::load_context_run_state(&state, &context_run_id)
                .await
                .expect("context run");
            if run.last_event_seq >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("context run journal persisted");

    let run = crate::http::context_runs::load_context_run_state(&state, &context_run_id)
        .await
        .expect("context run");
    assert_eq!(run.run_id, context_run_id);
    assert_eq!(run.run_type, "session");
    assert_eq!(
        run.status,
        crate::http::context_types::ContextRunStatus::Completed
    );
    assert_eq!(run.workspace.canonical_path, "/tmp/tandem-session-journal");
    assert!(run.started_at_ms.is_some());

    let app = app_router(state.clone());
    let events_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{context_run_id}/events"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(events_resp.status(), StatusCode::OK);
    let events_body = to_bytes(events_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let events_payload: Value = serde_json::from_slice(&events_body).expect("json");
    let events = events_payload
        .get("events")
        .and_then(|value| value.as_array())
        .expect("events array");
    let event_types = events
        .iter()
        .filter_map(|row| row.get("type").and_then(|value| value.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        event_types,
        vec![
            "session_run_started",
            "session_tool_updated",
            "session_run_finished",
        ]
    );

    task.abort();
}

#[tokio::test]
async fn concurrent_prompt_async_returns_conflict_with_nested_active_run() {
    let state = test_state().await;
    let session = Session::new(Some("conflict".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());

    let first_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async?return=run"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [
                    {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"block\"}]}"}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let first_resp = app.clone().oneshot(first_req).await.expect("response");
    assert_eq!(first_resp.status(), StatusCode::ACCEPTED);
    let first_body = to_bytes(first_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let first_payload: Value = serde_json::from_slice(&first_body).expect("json");
    let active_run_id = first_payload
        .get("runID")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(!active_run_id.is_empty());

    let second_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"parts":[{"type":"text","text":"second prompt"}]}).to_string(),
        ))
        .expect("request");
    let second_resp = app.clone().oneshot(second_req).await.expect("response");
    assert_eq!(second_resp.status(), StatusCode::CONFLICT);
    let second_body = to_bytes(second_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let second_payload: Value = serde_json::from_slice(&second_body).expect("json");
    assert_eq!(
        second_payload.get("code").and_then(|v| v.as_str()),
        Some("SESSION_RUN_CONFLICT")
    );
    assert_eq!(
        second_payload
            .get("activeRun")
            .and_then(|v| v.get("runID"))
            .and_then(|v| v.as_str()),
        Some(active_run_id.as_str())
    );
    assert!(second_payload
        .get("activeRun")
        .and_then(|v| v.get("startedAtMs"))
        .and_then(|v| v.as_i64())
        .is_some());
    assert!(second_payload
        .get("activeRun")
        .and_then(|v| v.get("lastActivityAtMs"))
        .and_then(|v| v.as_i64())
        .is_some());
    assert!(second_payload
        .get("retryAfterMs")
        .and_then(|v| v.as_u64())
        .is_some());
    assert_eq!(
        second_payload
            .get("attachEventStream")
            .and_then(|v| v.as_str()),
        Some(format!("/event?sessionID={session_id}&runID={active_run_id}").as_str())
    );

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/cancel"))
        .body(Body::empty())
        .expect("cancel request");
    let cancel_resp = app_router(state)
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn append_message_succeeds_while_run_is_active() {
    let state = test_state().await;
    let session = Session::new(Some("append-active".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());

    let first_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async?return=run"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [
                        {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"block append\"}]}"}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
    let first_resp = app.clone().oneshot(first_req).await.expect("response");
    assert_eq!(first_resp.status(), StatusCode::ACCEPTED);

    let append_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message?mode=append"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"parts":[{"type":"text","text":"appended while active"}]}).to_string(),
        ))
        .expect("append request");
    let append_resp = app.clone().oneshot(append_req).await.expect("response");
    assert_eq!(append_resp.status(), StatusCode::OK);
    let _ = to_bytes(append_resp.into_body(), usize::MAX)
        .await
        .expect("body");

    let list_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}/message"))
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
    let list = list_payload.as_array().cloned().unwrap_or_default();
    assert!(!list.is_empty());
    let has_appended_text = list.iter().any(|message| {
        message
            .get("parts")
            .and_then(|v| v.as_array())
            .map(|parts| {
                parts.iter().any(|part| {
                    part.get("text").and_then(|v| v.as_str()) == Some("appended while active")
                })
            })
            .unwrap_or(false)
    });
    assert!(has_appended_text);

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/cancel"))
        .body(Body::empty())
        .expect("cancel request");
    let cancel_resp = app_router(state)
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auto_rename_session_on_first_message() {
    let state = test_state().await;
    let app = app_router(state.clone());

    // 1. Create session
    let create_req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "title": null }).to_string()))
        .expect("create request");
    let create_resp = app.clone().oneshot(create_req).await.expect("response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let session_id = session
        .get("id")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();
    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .expect("title");
    assert_eq!(title, "New session");

    // 2. Append first message
    let append_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{"type": "text", "text": "Hello world this is a test message"}]
            })
            .to_string(),
        ))
        .expect("append request");
    let append_resp = app.clone().oneshot(append_req).await.expect("response");
    assert_eq!(append_resp.status(), StatusCode::OK);

    // 3. Verify title changed
    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .expect("title");
    assert_eq!(title, "Hello world this is a test message");

    // 4. Append second message
    let append_req_2 = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{"type": "text", "text": "Another message"}]
            })
            .to_string(),
        ))
        .expect("append request");
    let append_resp_2 = app.clone().oneshot(append_req_2).await.expect("response");
    assert_eq!(append_resp_2.status(), StatusCode::OK);

    // 5. Verify title did NOT change
    let get_req_2 = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp_2 = app.clone().oneshot(get_req_2).await.expect("response");

    let body = to_bytes(get_resp_2.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .expect("title");
    // Title should remain as the first message
    assert_eq!(title, "Hello world this is a test message");
}

#[tokio::test]
async fn auto_rename_ignores_memory_context_wrappers() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "title": null }).to_string()))
        .expect("create request");
    let create_resp = app.clone().oneshot(create_req).await.expect("response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let session_id = session
        .get("id")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();

    let wrapped = "<memory_context>\n<current_session>\n- fact\n</current_session>\n</memory_context>\n\n[Mode instructions]\nUse tools.\n\n[User request]\nShip the fix quickly";
    let append_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{"type":"text","text": wrapped}]
            })
            .to_string(),
        ))
        .expect("append request");
    let append_resp = app.clone().oneshot(append_req).await.expect("response");
    assert_eq!(append_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .expect("title");
    assert_eq!(title, "Ship the fix quickly");
}

#[tokio::test]
async fn get_config_redacts_channel_bot_token() {
    let state = test_state().await;
    let _ = state
        .config
        .patch_project(json!({
            "channels": {
                "telegram": {
                    "bot_token": "tg-secret",
                    "allowed_users": ["*"],
                    "mention_only": false
                }
            }
        }))
        .await
        .expect("patch project");
    let app = app_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/config")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(
        payload
            .get("effective")
            .and_then(|v| v.get("channels"))
            .and_then(|v| v.get("telegram"))
            .and_then(|v| v.get("bot_token"))
            .and_then(Value::as_str),
        Some("[REDACTED]")
    );
}

#[tokio::test]
async fn channel_session_archival_writes_deduped_global_exchange_memory() {
    let state = test_state().await;
    let mut session = Session::new(
        Some("telegram — @tester — dm:42".to_string()),
        Some(".".to_string()),
    );
    session.workspace_root = Some("/tmp/tandem-channel-workspace".to_string());
    session.project_id = Some("workspace-archival".to_string());
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");

    state
        .storage
        .append_message(
            &session_id,
            Message::new(
                MessageRole::User,
                vec![MessagePart::Text {
                    text: "Please remember that we are wiring channel exchanges into memory."
                        .to_string(),
                }],
            ),
        )
        .await
        .expect("append user");
    state
        .storage
        .append_message(
            &session_id,
            Message::new(
                MessageRole::Assistant,
                vec![MessagePart::Text {
                    text: "We now archive exact user and assistant exchanges into global memory."
                        .to_string(),
                }],
            ),
        )
        .await
        .expect("append assistant");

    crate::http::sessions::archive_session_exchange_to_global_memory(
        state.clone(),
        session_id.clone(),
    )
    .await;
    crate::http::sessions::archive_session_exchange_to_global_memory(
        state.clone(),
        session_id.clone(),
    )
    .await;

    let paths = tandem_core::resolve_shared_paths().expect("shared paths");
    let db = tandem_memory::db::MemoryDatabase::new(&paths.memory_db_path)
        .await
        .expect("memory db");
    let rows = db.get_global_chunks(20).await.expect("global chunks");
    let matches = rows
        .into_iter()
        .filter(|chunk| chunk.source == "chat_exchange")
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1);
    assert!(matches[0].content.contains("channel exchanges into memory"));
    assert!(matches[0]
        .content
        .contains("archive exact user and assistant exchanges"));
}
