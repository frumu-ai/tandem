use super::*;
use async_trait::async_trait;
use futures::stream;
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::{collections::VecDeque, time::Duration};
use tandem_providers::{ChatMessage, Provider, StreamChunk};
use tandem_types::{ModelInfo, ModelSpec, ProviderInfo, ToolMode, ToolSchema};
use tokio::sync::Mutex;
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
        _sampling: tandem_types::SamplingParams,
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

#[derive(Debug, Clone)]
enum StrictKbProviderStep {
    ToolCall { tool: String, args: Value },
    Text(String),
    StreamError(String),
    CompleteText(String),
}

struct ScriptedStrictKbProvider {
    steps: Arc<Mutex<VecDeque<StrictKbProviderStep>>>,
}

#[async_trait]
impl Provider for ScriptedStrictKbProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "strict-kb-test".to_string(),
            name: "Strict KB Test".to_string(),
            models: vec![ModelInfo {
                id: "strict-kb-test-1".to_string(),
                provider_id: "strict-kb-test".to_string(),
                display_name: "Strict KB Test 1".to_string(),
                context_window: 8192,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        let step = self
            .steps
            .lock()
            .await
            .pop_front()
            .expect("scripted strict KB provider complete step");
        match step {
            StrictKbProviderStep::CompleteText(text) | StrictKbProviderStep::Text(text) => Ok(text),
            StrictKbProviderStep::StreamError(error) => anyhow::bail!(error),
            StrictKbProviderStep::ToolCall { .. } => {
                anyhow::bail!("unexpected tool call step for completion")
            }
        }
    }

    async fn stream(
        &self,
        _messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let step = self
            .steps
            .lock()
            .await
            .pop_front()
            .expect("scripted strict KB provider step");
        let chunks = match step {
            StrictKbProviderStep::ToolCall { tool, args } => vec![
                Ok(StreamChunk::ToolCallStart {
                    id: "call_kb_1".to_string(),
                    name: tool,
                }),
                Ok(StreamChunk::ToolCallDelta {
                    id: "call_kb_1".to_string(),
                    args_delta: args.to_string(),
                }),
                Ok(StreamChunk::ToolCallEnd {
                    id: "call_kb_1".to_string(),
                }),
                Ok(StreamChunk::Done {
                    finish_reason: "tool_calls".to_string(),
                    usage: None,
                }),
            ],
            StrictKbProviderStep::Text(text) => vec![
                Ok(StreamChunk::TextDelta(text)),
                Ok(StreamChunk::Done {
                    finish_reason: "stop".to_string(),
                    usage: None,
                }),
            ],
            StrictKbProviderStep::StreamError(error) => vec![Err(anyhow::anyhow!(error))],
            StrictKbProviderStep::CompleteText(_) => {
                vec![Err(anyhow::anyhow!(
                    "unexpected completion step for stream"
                ))]
            }
        };
        Ok(Box::pin(stream::iter(chunks)))
    }
}

struct StaticKbTool {
    output: String,
}

fn tenant_request(
    method: &str,
    uri: impl Into<String>,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri.into())
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", actor_id);
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).expect("tenant request")
}

async fn create_tenant_session(
    app: axum::Router,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    title: &str,
) -> Value {
    let req = tenant_request(
        "POST",
        "/session",
        org_id,
        workspace_id,
        actor_id,
        Some(json!({
            "title": title,
            "directory": "."
        })),
    );
    let resp = app.oneshot(req).await.expect("create response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    serde_json::from_slice(&body).expect("created session")
}

async fn tenant_status(
    app: axum::Router,
    method: &str,
    uri: impl Into<String>,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    body: Option<Value>,
) -> StatusCode {
    let resp = app
        .oneshot(tenant_request(
            method,
            uri,
            org_id,
            workspace_id,
            actor_id,
            body,
        ))
        .await
        .expect("tenant response");
    resp.status()
}

#[async_trait]
impl tandem_tools::Tool for StaticKbTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "mcp.kb.search_documents",
            "Static KB search tool for tests",
            json!({
                "type": "object",
                "additionalProperties": true
            }),
        )
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<tandem_types::ToolResult> {
        Ok(tandem_types::ToolResult {
            output: self.output.clone(),
            metadata: json!({}),
        })
    }
}

async fn strict_kb_test_state(kb_output: &str, steps: Vec<StrictKbProviderStep>) -> AppState {
    let state = test_state().await;
    tokio::spawn(crate::run_session_part_persister(state.clone()));
    state
        .providers
        .replace_for_test(
            vec![Arc::new(ScriptedStrictKbProvider {
                steps: Arc::new(Mutex::new(VecDeque::from(steps))),
            })],
            Some("strict-kb-test".to_string()),
        )
        .await;
    state
        .tools
        .register_tool(
            "mcp.kb.search_documents".to_string(),
            Arc::new(StaticKbTool {
                output: kb_output.to_string(),
            }),
        )
        .await;
    state
        .tools
        .register_tool(
            "mcp.kb.answer_question".to_string(),
            Arc::new(StaticKbTool {
                output: kb_output.to_string(),
            }),
        )
        .await;
    state
        .mcp
        .add("kb".to_string(), "memory://kb".to_string())
        .await;
    assert!(
        state
            .mcp
            .set_grounding_metadata("kb", Some("knowledgebase".to_string()), Some(true))
            .await
    );
    state
}

async fn run_prompt_sync_messages(
    state: AppState,
    question: &str,
    strict_kb_grounding: bool,
) -> Vec<Value> {
    run_prompt_sync_messages_with_allowlist(
        state,
        question,
        strict_kb_grounding,
        json!(["mcp.kb.*"]),
    )
    .await
}

async fn run_prompt_sync_messages_with_allowlist(
    state: AppState,
    question: &str,
    strict_kb_grounding: bool,
    tool_allowlist: Value,
) -> Vec<Value> {
    let session = Session::new(Some("strict kb".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");
    state
        .engine_loop
        .set_session_auto_approve_permissions(&session_id, true)
        .await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_sync"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{ "type": "text", "text": question }],
                "model": {
                    "provider_id": "strict-kb-test",
                    "model_id": "strict-kb-test-1"
                },
                "tool_allowlist": tool_allowlist,
                "strict_kb_grounding": strict_kb_grounding
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice::<Vec<Value>>(&body).expect("prompt_sync messages")
}

#[tokio::test]
async fn tenant_a_cannot_access_tenant_b_session_routes() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let session_a = create_tenant_session(
        app.clone(),
        "org-a",
        "workspace-a",
        "user-a",
        "tenant a session",
    )
    .await;
    let session_b = create_tenant_session(
        app.clone(),
        "org-b",
        "workspace-b",
        "user-b",
        "tenant b session",
    )
    .await;
    let session_a_id = session_a["id"].as_str().expect("session a id").to_string();
    let session_b_id = session_b["id"].as_str().expect("session b id").to_string();

    state
        .storage
        .append_message(
            &session_b_id,
            Message::new(
                MessageRole::User,
                vec![MessagePart::Text {
                    text: "tenant b secret".to_string(),
                }],
            ),
        )
        .await
        .expect("append tenant b message");

    let list_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            "/session?scope=global&page_size=50",
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let listed: Vec<Value> = serde_json::from_slice(&list_body).expect("session list");
    let listed_ids = listed
        .iter()
        .filter_map(|session| session.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(listed_ids.contains(&session_a_id.as_str()));
    assert!(!listed_ids.contains(&session_b_id.as_str()));

    let status_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            "/session/status",
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("status response");
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body = to_bytes(status_resp.into_body(), usize::MAX)
        .await
        .expect("status body");
    let status_payload: Value = serde_json::from_slice(&status_body).expect("status json");
    assert!(status_payload.get(&session_a_id).is_some());
    assert!(status_payload.get(&session_b_id).is_none());

    for (method, uri, body) in [
        ("GET", format!("/session/{session_b_id}"), None),
        ("GET", format!("/session/{session_b_id}/message"), None),
        ("GET", format!("/session/{session_b_id}/todo"), None),
        ("GET", format!("/session/{session_b_id}/run"), None),
        ("GET", format!("/session/{session_b_id}/diff"), None),
        ("GET", format!("/session/{session_b_id}/children"), None),
        (
            "POST",
            format!("/session/{session_b_id}/message"),
            Some(json!({"parts":[{"type":"text","text":"nope"}]})),
        ),
        (
            "POST",
            format!("/session/{session_b_id}/prompt_async"),
            Some(json!({"parts":[{"type":"text","text":"nope"}]})),
        ),
        (
            "POST",
            format!("/session/{session_b_id}/prompt_sync"),
            Some(json!({"parts":[{"type":"text","text":"nope"}]})),
        ),
        (
            "POST",
            format!("/session/{session_b_id}/attach"),
            Some(json!({"target_workspace":"/tmp/tenant-a"})),
        ),
        (
            "POST",
            format!("/session/{session_b_id}/workspace/override"),
            Some(json!({"ttl_seconds":60})),
        ),
        (
            "PATCH",
            format!("/session/{session_b_id}"),
            Some(json!({"title":"stolen"})),
        ),
        ("POST", format!("/session/{session_b_id}/abort"), None),
        (
            "POST",
            format!("/session/{session_b_id}/run/run-b/cancel"),
            None,
        ),
        ("POST", format!("/session/{session_b_id}/fork"), None),
        ("POST", format!("/session/{session_b_id}/revert"), None),
        ("POST", format!("/session/{session_b_id}/unrevert"), None),
        ("POST", format!("/session/{session_b_id}/share"), None),
        ("DELETE", format!("/session/{session_b_id}/share"), None),
        ("POST", format!("/session/{session_b_id}/summarize"), None),
        ("DELETE", format!("/session/{session_b_id}"), None),
    ] {
        let status = tenant_status(
            app.clone(),
            method,
            uri,
            "org-a",
            "workspace-a",
            "user-a",
            body,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} should be tenant-hidden"
        );
    }

    let tenant_b_session = state
        .storage
        .get_session(&session_b_id)
        .await
        .expect("tenant b session still exists");
    assert_eq!(tenant_b_session.title, "tenant b session");
    assert_eq!(tenant_b_session.messages.len(), 1);
}

#[tokio::test]
async fn tenant_event_stream_filters_other_tenant_events() {
    let tenant_a = TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        Some("deployment-a".to_string()),
        "user-a",
    );

    let tenant_b_event = EngineEvent::new(
        "session.updated",
        json!({
            "sessionID": "session-b",
            "tenantContext": TenantContext::explicit_user_workspace(
                "org-b",
                "workspace-b",
                Some("deployment-b".to_string()),
                "user-b",
            )
        }),
    );
    let tenant_a_event = EngineEvent::new(
        "session.updated",
        json!({
            "sessionID": "session-a",
            "tenantContext": TenantContext::explicit_user_workspace(
                "org-a",
                "workspace-a",
                Some("deployment-a".to_string()),
                "user-a",
            )
        }),
    );

    assert!(!super::super::global::event_visible_to_tenant(
        &tenant_b_event,
        &tenant_a
    ));
    assert!(super::super::global::event_visible_to_tenant(
        &tenant_a_event,
        &tenant_a
    ));
}

#[tokio::test]
async fn prompt_sync_channel_user_eleventh_request_returns_429_retry_after() {
    let state = test_state().await;
    let mut session = Session::new(Some("channel limited".to_string()), Some(".".to_string()));
    session.source_kind = Some("channel".to_string());
    session.source_metadata = Some(json!({
        "channel": "telegram",
        "user_id": "42"
    }));
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");

    let key = crate::app::rate_limit::ChannelRateLimitKey {
        channel: "telegram".to_string(),
        user_id: "42".to_string(),
    };
    for _ in 0..10 {
        assert!(
            state
                .channel_rate_limiter
                .check(
                    &key,
                    crate::app::rate_limit::ChannelRateLimitKind::Prompt,
                    tandem_channels::config::ChannelSecurityProfile::PublicDemo,
                )
                .await
                .allowed
        );
    }

    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_sync"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"parts": []}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");

    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(resp.headers().contains_key(header::RETRY_AFTER));
}

fn latest_assistant_text(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| {
            message
                .get("info")
                .and_then(|info| info.get("role"))
                .and_then(Value::as_str)
                == Some("assistant")
        })
        .and_then(|message| message.get("parts").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
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
async fn create_session_applies_deny_permission_rules_and_ignores_invalid_entries() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "title": "deny permissions",
                "directory": ".",
                "permission": [
                    {"permission": "todo_write", "pattern": "todo_write", "action": "deny"},
                    {"permission": "mcp*", "pattern": "*", "action": "reject"},
                    {"permission": "ignored_missing_action", "pattern": "*"},
                    {"permission": "", "pattern": "*", "action": "deny"},
                    {"permission": "ignored_bad_action", "pattern": "*", "action": "never"}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let todo_action = state.permissions.evaluate("todo_write", "todo_write").await;
    assert!(matches!(todo_action, tandem_core::PermissionAction::Deny));
    let mcp_action = state.permissions.evaluate("mcp_list", "anything").await;
    assert!(matches!(mcp_action, tandem_core::PermissionAction::Deny));
    let ignored_action = state
        .permissions
        .evaluate("ignored_bad_action", "anything")
        .await;
    assert!(matches!(ignored_action, tandem_core::PermissionAction::Ask));
    assert_eq!(state.permissions.list_rules().await.len(), 2);
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
                "state": "failed",
                "error": "WRITE_ARGS_EMPTY_FROM_PROVIDER"
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
                "state": "failed",
                "error": "WRITE_ARGS_EMPTY_FROM_PROVIDER"
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
async fn list_sessions_omits_message_history_but_get_session_keeps_it() {
    let state = test_state().await;
    let mut session = Session::new(Some("summary-only".to_string()), Some(".".to_string()));
    session.messages.push(Message::new(
        MessageRole::Assistant,
        vec![MessagePart::Text {
            text: "large transcript payload".repeat(1_000),
        }],
    ));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());

    let list_req = Request::builder()
        .method("GET")
        .uri("/session?scope=global")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    let listed = list_payload
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(session_id.as_str()))
        })
        .expect("listed session");
    assert_eq!(listed.get("messages").and_then(Value::as_array).unwrap().len(), 0);

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload
            .get("messages")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
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
async fn create_session_local_mode_does_not_require_hosted_auth() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "local-mode",
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
    assert_eq!(stored_session.tenant_context.org_id, "local");
    assert_eq!(stored_session.tenant_context.workspace_id, "local");
    assert!(stored_session.tenant_context.actor_id.is_none());
    assert!(stored_session.verified_tenant_context.is_none());
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
async fn session_listing_filters_chat_source_from_automation_source() {
    let state = test_state().await;
    let mut chat = Session::new(Some("Operator chat".to_string()), Some(".".to_string()));
    chat.source_kind = Some("chat".to_string());
    state.storage.save_session(chat).await.expect("save chat");

    let automation = Session::new(
        Some(
            "Automation automation-v2-bug-monitor-triage-failure-draft-1 / inspect_failure_report"
                .to_string(),
        ),
        Some(".".to_string()),
    );
    state
        .storage
        .save_session(automation)
        .await
        .expect("save automation");

    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/session?scope=global&source=chat")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let rows = payload.as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("title").and_then(Value::as_str),
        Some("Operator chat")
    );
    assert_eq!(
        rows[0].get("sourceKind").and_then(Value::as_str),
        Some("chat")
    );
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
    state
        .providers
        .replace_for_test(
            vec![Arc::new(StreamedWriteTestProvider)],
            Some("streamed-test".to_string()),
        )
        .await;
    let mut session = Session::new(Some("sse-shape".to_string()), Some(".".to_string()));
    session.model = Some(ModelSpec {
        provider_id: "streamed-test".to_string(),
        model_id: "streamed-test-1".to_string(),
    });
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts":[{"type":"text","text":"hello streaming"}],
                "model": {
                    "provider_id": "streamed-test",
                    "model_id": "streamed-test-1"
                },
                "tool_mode": "required"
            })
            .to_string(),
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
                strict_kb_grounding: None,
                context_mode: None,
                write_required: None,
                prewrite_requirements: None,
                sampling: Default::default(),
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

/// GOV-B2d: aborting a session is attributed to the calling actor and written to
/// the protected audit log.
#[tokio::test]
async fn abort_session_writes_attributed_protected_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "title": "b2d", "directory": "." }).to_string()))
        .expect("create request");
    let create_resp = app.clone().oneshot(create).await.expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let body = to_bytes(create_resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let session_id = payload.get("id").and_then(Value::as_str).expect("session id").to_string();

    let abort = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/abort"))
        .header("x-tandem-actor-id", "operator-b2d")
        .body(Body::empty())
        .expect("abort request");
    let abort_resp = app.clone().oneshot(abort).await.expect("abort response");
    assert_eq!(abort_resp.status(), StatusCode::OK);

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"session.aborted\""));
    assert!(audit.contains("operator-b2d"));
    assert!(audit.contains(&session_id));
}
