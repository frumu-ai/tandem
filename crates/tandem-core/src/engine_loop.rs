use futures::StreamExt;
use serde_json::{json, Value};
use tandem_providers::{ChatMessage, ProviderRegistry, StreamChunk};
use tandem_tools::ToolRegistry;
use tandem_types::{
    EngineEvent, Message, MessagePart, MessagePartInput, MessageRole, SendMessageRequest,
};
use tandem_wire::WireMessagePart;
use tokio_util::sync::CancellationToken;

use crate::{
    AgentDefinition, AgentRegistry, CancellationRegistry, EventBus, PermissionAction,
    PermissionManager, PluginRegistry, Storage,
};

#[derive(Clone)]
pub struct EngineLoop {
    storage: std::sync::Arc<Storage>,
    event_bus: EventBus,
    providers: ProviderRegistry,
    plugins: PluginRegistry,
    agents: AgentRegistry,
    permissions: PermissionManager,
    tools: ToolRegistry,
    cancellations: CancellationRegistry,
}

impl EngineLoop {
    pub fn new(
        storage: std::sync::Arc<Storage>,
        event_bus: EventBus,
        providers: ProviderRegistry,
        plugins: PluginRegistry,
        agents: AgentRegistry,
        permissions: PermissionManager,
        tools: ToolRegistry,
        cancellations: CancellationRegistry,
    ) -> Self {
        Self {
            storage,
            event_bus,
            providers,
            plugins,
            agents,
            permissions,
            tools,
            cancellations,
        }
    }

    pub async fn run_prompt_async(
        &self,
        session_id: String,
        req: SendMessageRequest,
    ) -> anyhow::Result<()> {
        let cancel = self.cancellations.create(&session_id).await;
        self.event_bus.publish(EngineEvent::new(
            "session.status",
            json!({"sessionID": session_id, "status":"running"}),
        ));
        let text = req
            .parts
            .iter()
            .map(|p| match p {
                MessagePartInput::Text { text } => text.clone(),
                MessagePartInput::File {
                    mime,
                    filename,
                    url,
                } => format!(
                    "[file mime={} name={} url={}]",
                    mime,
                    filename.clone().unwrap_or_else(|| "unknown".to_string()),
                    url
                ),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let active_agent = self.agents.get(req.agent.as_deref()).await;

        let user_message = Message::new(
            MessageRole::User,
            vec![MessagePart::Text { text: text.clone() }],
        );
        let user_message_id = user_message.id.clone();
        self.storage
            .append_message(&session_id, user_message)
            .await?;

        let user_part = WireMessagePart::text(&session_id, &user_message_id, text.clone());
        self.event_bus.publish(EngineEvent::new(
            "message.part.updated",
            json!({
                "part": user_part,
                "delta": text,
                "agent": active_agent.name
            }),
        ));

        if cancel.is_cancelled() {
            self.event_bus.publish(EngineEvent::new(
                "session.status",
                json!({"sessionID": session_id, "status":"cancelled"}),
            ));
            self.cancellations.remove(&session_id).await;
            return Ok(());
        }

        let completion = if let Some((tool, args)) = parse_tool_invocation(&text) {
            if !agent_can_use_tool(&active_agent, &tool) {
                format!(
                    "Tool `{tool}` is not enabled for agent `{}`.",
                    active_agent.name
                )
            } else {
                self.execute_tool_with_permission(
                    &session_id,
                    &user_message_id,
                    tool.clone(),
                    args,
                    cancel.clone(),
                )
                .await?
                .unwrap_or_default()
            }
        } else {
            let mut completion = String::new();
            let mut max_iterations = 25usize;
            let mut followup_context: Option<String> = None;

            while max_iterations > 0 && !cancel.is_cancelled() {
                max_iterations -= 1;
                let mut messages = load_chat_history(self.storage.clone(), &session_id).await;
                if let Some(system) = active_agent.system_prompt.as_ref() {
                    messages.insert(
                        0,
                        ChatMessage {
                            role: "system".to_string(),
                            content: system.clone(),
                        },
                    );
                }
                if let Some(extra) = followup_context.take() {
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: extra,
                    });
                }
                let stream = self
                    .providers
                    .default_stream(messages, Some(self.tools.list().await), cancel.clone())
                    .await?;
                tokio::pin!(stream);
                completion.clear();
                while let Some(chunk) = stream.next().await {
                    let Ok(chunk) = chunk else {
                        continue;
                    };
                    match chunk {
                        StreamChunk::TextDelta(delta) => {
                            completion.push_str(&delta);
                            let delta = truncate_text(&delta, 4_000);
                            let delta_part =
                                WireMessagePart::text(&session_id, &user_message_id, delta.clone());
                            self.event_bus.publish(EngineEvent::new(
                                "message.part.updated",
                                json!({"part": delta_part, "delta": delta}),
                            ));
                        }
                        StreamChunk::ReasoningDelta(_reasoning) => {}
                        StreamChunk::Done { .. } => break,
                        _ => {}
                    }
                    if cancel.is_cancelled() {
                        break;
                    }
                }

                if let Some((tool, args)) = parse_tool_invocation_from_response(&completion) {
                    if !agent_can_use_tool(&active_agent, &tool) {
                        break;
                    }
                    let Some(output) = self
                        .execute_tool_with_permission(
                            &session_id,
                            &user_message_id,
                            tool.clone(),
                            args,
                            cancel.clone(),
                        )
                        .await?
                    else {
                        break;
                    };
                    followup_context = Some(format!("{output}\nContinue."));
                    continue;
                }

                break;
            }
            truncate_text(&completion, 16_000)
        };
        if cancel.is_cancelled() {
            self.event_bus.publish(EngineEvent::new(
                "session.status",
                json!({"sessionID": session_id, "status":"cancelled"}),
            ));
            self.cancellations.remove(&session_id).await;
            return Ok(());
        }
        let assistant = Message::new(
            MessageRole::Assistant,
            vec![MessagePart::Text {
                text: completion.clone(),
            }],
        );
        let assistant_message_id = assistant.id.clone();
        self.storage.append_message(&session_id, assistant).await?;
        let final_part = WireMessagePart::text(
            &session_id,
            &assistant_message_id,
            truncate_text(&completion, 16_000),
        );
        self.event_bus.publish(EngineEvent::new(
            "message.part.updated",
            json!({"part": final_part}),
        ));
        self.event_bus.publish(EngineEvent::new(
            "session.updated",
            json!({"sessionID": session_id, "status":"idle"}),
        ));
        self.event_bus.publish(EngineEvent::new(
            "session.status",
            json!({"sessionID": session_id, "status":"idle"}),
        ));
        self.cancellations.remove(&session_id).await;
        Ok(())
    }

    pub async fn run_oneshot(&self, prompt: String) -> anyhow::Result<String> {
        self.providers.default_complete(&prompt).await
    }

    async fn execute_tool_with_permission(
        &self,
        session_id: &str,
        message_id: &str,
        tool: String,
        args: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<Option<String>> {
        let rule = self
            .plugins
            .permission_override(&tool)
            .await
            .unwrap_or(self.permissions.evaluate(&tool, &tool).await);
        if matches!(rule, PermissionAction::Deny) {
            return Ok(Some(format!(
                "Permission denied for tool `{tool}` by policy."
            )));
        }

        let mut effective_args = args.clone();
        if matches!(rule, PermissionAction::Ask) {
            let pending = self
                .permissions
                .ask_for_session(Some(session_id), &tool, args.clone())
                .await;
            let mut pending_part = WireMessagePart::tool_invocation(
                session_id,
                message_id,
                tool.clone(),
                args.clone(),
            );
            pending_part.id = Some(pending.id.clone());
            pending_part.state = Some("pending".to_string());
            self.event_bus.publish(EngineEvent::new(
                "message.part.updated",
                json!({"part": pending_part}),
            ));
            let reply = self
                .permissions
                .wait_for_reply(&pending.id, cancel.clone())
                .await;
            if cancel.is_cancelled() {
                return Ok(None);
            }
            let approved = matches!(reply.as_deref(), Some("once" | "always" | "allow"));
            if !approved {
                let mut denied_part =
                    WireMessagePart::tool_result(session_id, message_id, tool.clone(), json!(null));
                denied_part.id = Some(pending.id);
                denied_part.state = Some("denied".to_string());
                denied_part.error = Some("Permission denied by user".to_string());
                self.event_bus.publish(EngineEvent::new(
                    "message.part.updated",
                    json!({"part": denied_part}),
                ));
                return Ok(Some(format!(
                    "Permission denied for tool `{tool}` by user."
                )));
            }
            effective_args = args;
        }

        let args = self.plugins.inject_tool_args(&tool, effective_args).await;
        let invoke_part =
            WireMessagePart::tool_invocation(session_id, message_id, tool.clone(), args.clone());
        self.event_bus.publish(EngineEvent::new(
            "message.part.updated",
            json!({"part": invoke_part}),
        ));
        let result = self
            .tools
            .execute_with_cancel(&tool, args, cancel.clone())
            .await?;
        emit_tool_side_events(
            self.storage.clone(),
            &self.event_bus,
            session_id,
            message_id,
            &tool,
            &result.metadata,
        )
        .await;
        let output = self.plugins.transform_tool_output(result.output).await;
        let output = truncate_text(&output, 16_000);
        let result_part = WireMessagePart::tool_result(
            session_id,
            message_id,
            tool.clone(),
            json!(output.clone()),
        );
        self.event_bus.publish(EngineEvent::new(
            "message.part.updated",
            json!({"part": result_part}),
        ));
        Ok(Some(truncate_text(
            &format!("Tool `{tool}` result:\n{output}"),
            16_000,
        )))
    }
}

fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut out = input[..max_len].to_string();
    out.push_str("...<truncated>");
    out
}

fn agent_can_use_tool(agent: &AgentDefinition, tool_name: &str) -> bool {
    match agent.tools.as_ref() {
        None => true,
        Some(list) => list.iter().any(|t| t == tool_name),
    }
}

fn parse_tool_invocation(input: &str) -> Option<(String, serde_json::Value)> {
    let raw = input.trim();
    if !raw.starts_with("/tool ") {
        return None;
    }
    let rest = raw.trim_start_matches("/tool ").trim();
    let mut split = rest.splitn(2, ' ');
    let tool = split.next()?.trim().to_string();
    let args = split
        .next()
        .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
        .unwrap_or_else(|| json!({}));
    Some((tool, args))
}

fn parse_tool_invocation_from_response(input: &str) -> Option<(String, serde_json::Value)> {
    let parsed = serde_json::from_str::<serde_json::Value>(input).ok()?;
    let tool = parsed.get("tool")?.as_str()?.to_string();
    let args = parsed.get("args").cloned().unwrap_or_else(|| json!({}));
    Some((tool, args))
}

async fn load_chat_history(storage: std::sync::Arc<Storage>, session_id: &str) -> Vec<ChatMessage> {
    let Some(session) = storage.get_session(session_id).await else {
        return Vec::new();
    };
    let messages = session
        .messages
        .into_iter()
        .map(|m| {
            let role = format!("{:?}", m.role).to_lowercase();
            let content = m
                .parts
                .into_iter()
                .filter_map(|part| match part {
                    MessagePart::Text { text } => Some(text),
                    MessagePart::Reasoning { text } => Some(text),
                    MessagePart::ToolInvocation { tool, result, .. } => Some(format!(
                        "Tool {tool} => {}",
                        result.unwrap_or_else(|| json!({}))
                    )),
                })
                .collect::<Vec<_>>()
                .join("\n");
            ChatMessage { role, content }
        })
        .collect::<Vec<_>>();
    compact_chat_history(messages)
}

async fn emit_tool_side_events(
    storage: std::sync::Arc<Storage>,
    bus: &EventBus,
    session_id: &str,
    message_id: &str,
    tool: &str,
    metadata: &serde_json::Value,
) {
    if tool == "todo_write" {
        let todos = metadata
            .get("todos")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let _ = storage.set_todos(session_id, todos.clone()).await;
        let normalized = storage.get_todos(session_id).await;
        bus.publish(EngineEvent::new(
            "todo.updated",
            json!({
                "sessionID": session_id,
                "todos": normalized
            }),
        ));
    }
    if tool == "question" {
        let questions = metadata
            .get("questions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let request = storage
            .add_question_request(session_id, message_id, questions.clone())
            .await
            .ok();
        bus.publish(EngineEvent::new(
            "question.asked",
            json!({
                "id": request
                    .as_ref()
                    .map(|req| req.id.clone())
                    .unwrap_or_else(|| format!("q-{}", uuid::Uuid::new_v4())),
                "sessionID": session_id,
                "messageID": message_id,
                "questions": questions,
                "tool": request.and_then(|req| {
                    req.tool.map(|tool| {
                        json!({
                            "callID": tool.call_id,
                            "messageID": tool.message_id
                        })
                    })
                })
            }),
        ));
    }
}

fn compact_chat_history(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    const MAX_CONTEXT_CHARS: usize = 80_000;
    const KEEP_RECENT_MESSAGES: usize = 40;

    if messages.len() <= KEEP_RECENT_MESSAGES {
        let total_chars = messages.iter().map(|m| m.content.len()).sum::<usize>();
        if total_chars <= MAX_CONTEXT_CHARS {
            return messages;
        }
    }

    let mut kept = messages;
    let mut dropped_count = 0usize;
    let mut total_chars = kept.iter().map(|m| m.content.len()).sum::<usize>();

    while kept.len() > KEEP_RECENT_MESSAGES || total_chars > MAX_CONTEXT_CHARS {
        if kept.is_empty() {
            break;
        }
        let removed = kept.remove(0);
        total_chars = total_chars.saturating_sub(removed.content.len());
        dropped_count += 1;
    }

    if dropped_count > 0 {
        kept.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: format!(
                    "[history compacted: omitted {} older messages to fit context window]",
                    dropped_count
                ),
            },
        );
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventBus, Storage};
    use uuid::Uuid;

    #[tokio::test]
    async fn todo_updated_event_is_normalized() {
        let base = std::env::temp_dir().join(format!("engine-loop-test-{}", Uuid::new_v4()));
        let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
        let session = tandem_types::Session::new(Some("s".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        emit_tool_side_events(
            storage.clone(),
            &bus,
            &session_id,
            "m1",
            "todo_write",
            &json!({"todos":[{"content":"ship parity"}]}),
        )
        .await;

        let event = rx.recv().await.expect("event");
        assert_eq!(event.event_type, "todo.updated");
        let todos = event
            .properties
            .get("todos")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(todos.len(), 1);
        assert!(todos[0].get("id").and_then(|v| v.as_str()).is_some());
        assert_eq!(
            todos[0].get("content").and_then(|v| v.as_str()),
            Some("ship parity")
        );
        assert!(todos[0].get("status").and_then(|v| v.as_str()).is_some());
    }

    #[tokio::test]
    async fn question_asked_event_contains_tool_reference() {
        let base = std::env::temp_dir().join(format!("engine-loop-test-{}", Uuid::new_v4()));
        let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
        let session = tandem_types::Session::new(Some("s".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        emit_tool_side_events(
            storage,
            &bus,
            &session_id,
            "msg-1",
            "question",
            &json!({"questions":[{"header":"Topic","question":"Pick one","options":[{"label":"A","description":"d"}]}]}),
        )
        .await;

        let event = rx.recv().await.expect("event");
        assert_eq!(event.event_type, "question.asked");
        assert_eq!(
            event
                .properties
                .get("sessionID")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            session_id
        );
        let tool = event
            .properties
            .get("tool")
            .cloned()
            .unwrap_or_else(|| json!({}));
        assert!(tool.get("callID").and_then(|v| v.as_str()).is_some());
        assert_eq!(
            tool.get("messageID").and_then(|v| v.as_str()),
            Some("msg-1")
        );
    }

    #[test]
    fn compact_chat_history_keeps_recent_and_inserts_summary() {
        let mut messages = Vec::new();
        for i in 0..60 {
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: format!("message-{i}"),
            });
        }
        let compacted = compact_chat_history(messages);
        assert!(compacted.len() <= 41);
        assert_eq!(compacted[0].role, "system");
        assert!(compacted[0].content.contains("history compacted"));
        assert!(compacted.iter().any(|m| m.content.contains("message-59")));
    }
}
