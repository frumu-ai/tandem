use uuid::Uuid;

use serde_json::{json, Value};
use tandem_providers::{ChatAttachment, ChatMessage};
use tandem_wire::WireMessagePart;

use crate::{EventBus, Storage};
use tandem_types::{EngineEvent, MessagePart, MessagePartInput};

use super::extract_todo_candidates_from_text;

pub(super) async fn emit_plan_todo_fallback(
    storage: std::sync::Arc<Storage>,
    bus: &EventBus,
    session_id: &str,
    message_id: &str,
    completion: &str,
) {
    let todos = extract_todo_candidates_from_text(completion);
    if todos.is_empty() {
        return;
    }

    let invoke_part = WireMessagePart::tool_invocation(
        session_id,
        message_id,
        "todo_write",
        json!({"todos": todos.clone()}),
    );
    let call_id = invoke_part.id.clone();
    bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({"part": invoke_part}),
    ));

    if storage.set_todos(session_id, todos.clone()).await.is_err() {
        let mut failed_part = WireMessagePart::tool_result(
            session_id,
            message_id,
            "todo_write",
            Some(json!({"todos": todos.clone()})),
            json!(null),
        );
        failed_part.id = call_id;
        failed_part.state = Some("failed".to_string());
        failed_part.error = Some("failed to persist plan todos".to_string());
        bus.publish(EngineEvent::new(
            "message.part.updated",
            json!({"part": failed_part}),
        ));
        return;
    }

    let normalized = storage.get_todos(session_id).await;
    let mut result_part = WireMessagePart::tool_result(
        session_id,
        message_id,
        "todo_write",
        Some(json!({"todos": todos.clone()})),
        json!({ "todos": normalized }),
    );
    result_part.id = call_id;
    bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({"part": result_part}),
    ));
    bus.publish(EngineEvent::new(
        "todo.updated",
        json!({
            "sessionID": session_id,
            "todos": normalized
        }),
    ));
}

pub(super) async fn emit_plan_question_fallback(
    storage: std::sync::Arc<Storage>,
    bus: &EventBus,
    session_id: &str,
    message_id: &str,
    completion: &str,
) {
    let trimmed = completion.trim();
    if trimmed.is_empty() {
        return;
    }

    let hints = extract_todo_candidates_from_text(trimmed)
        .into_iter()
        .take(6)
        .filter_map(|v| {
            v.get("content")
                .and_then(|c| c.as_str())
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();

    let mut options = hints
        .iter()
        .map(|label| json!({"label": label, "description": "Use this as a starting task"}))
        .collect::<Vec<_>>();
    if options.is_empty() {
        options = vec![
            json!({"label":"Define scope", "description":"Clarify the intended outcome"}),
            json!({"label":"Provide constraints", "description":"Budget, timeline, and constraints"}),
            json!({"label":"Draft a starter list", "description":"Generate a first-pass task list"}),
        ];
    }

    let question_payload = vec![json!({
        "header":"Planning Input",
        "question":"I couldn't produce a concrete task list yet. Which tasks should I include first?",
        "options": options,
        "multiple": true,
        "custom": true
    })];

    let request = storage
        .add_question_request(session_id, message_id, question_payload.clone())
        .await
        .ok();
    bus.publish(EngineEvent::new(
        "question.asked",
        json!({
            "id": request
                .as_ref()
                .map(|req| req.id.clone())
                .unwrap_or_else(|| format!("q-{}", Uuid::new_v4())),
            "sessionID": session_id,
            "messageID": message_id,
            "questions": question_payload,
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

#[derive(Debug, Clone, Copy)]
pub(super) enum ChatHistoryProfile {
    Full,
    Standard,
    Compact,
}

pub(super) async fn load_chat_history(
    storage: std::sync::Arc<Storage>,
    session_id: &str,
    profile: ChatHistoryProfile,
) -> Vec<ChatMessage> {
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
                .map(|part| match part {
                    MessagePart::Text { text } => text,
                    MessagePart::Reasoning { text } => text,
                    MessagePart::ToolInvocation {
                        tool,
                        args,
                        result,
                        error,
                    } => summarize_tool_invocation_for_history(
                        &tool,
                        &args,
                        result.as_ref(),
                        error.as_deref(),
                    ),
                })
                .collect::<Vec<_>>()
                .join("\n");
            ChatMessage {
                role,
                content,
                attachments: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    compact_chat_history(messages, profile)
}

fn summarize_tool_invocation_for_history(
    tool: &str,
    args: &Value,
    result: Option<&Value>,
    error: Option<&str>,
) -> String {
    let mut segments = vec![format!("Tool {tool}")];
    if !args.is_null()
        && !args.as_object().is_some_and(|value| value.is_empty())
        && !args
            .as_str()
            .map(|value| value.trim().is_empty())
            .unwrap_or(false)
    {
        segments.push(format!("args={args}"));
    }
    if let Some(error) = error.map(str::trim).filter(|value| !value.is_empty()) {
        segments.push(format!("error={error}"));
    }
    if let Some(result) = result.filter(|value| !value.is_null()) {
        segments.push(format!("result={result}"));
    }
    if segments.len() == 1 {
        segments.push("result={}".to_string());
    }
    segments.join(" ")
}

pub(super) fn attach_to_last_user_message(
    messages: &mut [ChatMessage],
    attachments: &[ChatAttachment],
) {
    if attachments.is_empty() {
        return;
    }
    if let Some(message) = messages.iter_mut().rev().find(|m| m.role == "user") {
        message.attachments = attachments.to_vec();
    }
}

pub(super) async fn build_runtime_attachments(
    provider_id: &str,
    parts: &[MessagePartInput],
) -> Vec<ChatAttachment> {
    if !supports_image_attachments(provider_id) {
        return Vec::new();
    }

    let mut attachments = Vec::new();
    for part in parts {
        let MessagePartInput::File { mime, url, .. } = part else {
            continue;
        };
        if !mime.to_ascii_lowercase().starts_with("image/") {
            continue;
        }
        if let Some(source_url) = normalize_attachment_source_url(url, mime).await {
            attachments.push(ChatAttachment::ImageUrl { url: source_url });
        }
    }

    attachments
}

pub(super) fn supports_image_attachments(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "openai"
            | "openai-codex"
            | "openrouter"
            | "ollama"
            | "groq"
            | "mistral"
            | "together"
            | "azure"
            | "bedrock"
            | "vertex"
            | "copilot"
    )
}

pub(super) async fn normalize_attachment_source_url(url: &str, mime: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("data:")
    {
        return Some(trimmed.to_string());
    }

    let file_path = trimmed
        .strip_prefix("file://")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(trimmed));
    if !file_path.exists() {
        return None;
    }

    let max_bytes = std::env::var("TANDEM_CHANNEL_MAX_ATTACHMENT_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(20 * 1024 * 1024);

    let bytes = match tokio::fs::read(&file_path).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!(
                "failed reading local attachment '{}': {}",
                file_path.to_string_lossy(),
                err
            );
            return None;
        }
    };
    if bytes.len() > max_bytes {
        tracing::warn!(
            "local attachment '{}' exceeds max bytes ({} > {})",
            file_path.to_string_lossy(),
            bytes.len(),
            max_bytes
        );
        return None;
    }

    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

pub(super) struct ToolSideEventContext<'a> {
    pub(super) session_id: &'a str,
    pub(super) message_id: &'a str,
    pub(super) tool: &'a str,
    pub(super) args: &'a serde_json::Value,
    pub(super) metadata: &'a serde_json::Value,
    pub(super) workspace_root: Option<&'a str>,
    pub(super) effective_cwd: Option<&'a str>,
}

pub(super) async fn emit_tool_side_events(
    storage: std::sync::Arc<Storage>,
    bus: &EventBus,
    ctx: ToolSideEventContext<'_>,
) {
    let ToolSideEventContext {
        session_id,
        message_id,
        tool,
        args,
        metadata,
        workspace_root,
        effective_cwd,
    } = ctx;
    if tool == "todo_write" {
        let todos_from_metadata = metadata
            .get("todos")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if !todos_from_metadata.is_empty() {
            let _ = storage.set_todos(session_id, todos_from_metadata).await;
        } else {
            let current = storage.get_todos(session_id).await;
            if let Some(updated) = apply_todo_updates_from_args(current, args) {
                let _ = storage.set_todos(session_id, updated).await;
            }
        }

        let normalized = storage.get_todos(session_id).await;
        bus.publish(EngineEvent::new(
            "todo.updated",
            json!({
                "sessionID": session_id,
                "todos": normalized,
                "workspaceRoot": workspace_root,
                "effectiveCwd": effective_cwd
            }),
        ));
    }
    if tool == "question" {
        let questions = metadata
            .get("questions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if questions.is_empty() {
            tracing::warn!(
                "question tool produced empty questions payload; skipping question.asked event session_id={} message_id={}",
                session_id,
                message_id
            );
        } else {
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
                    }),
                    "workspaceRoot": workspace_root,
                    "effectiveCwd": effective_cwd
                }),
            ));
        }
    }
    if let Some(events) = metadata.get("events").and_then(|v| v.as_array()) {
        for event in events {
            let Some(event_type) = event.get("type").and_then(|v| v.as_str()) else {
                continue;
            };
            if !event_type.starts_with("agent_team.") {
                continue;
            }
            let mut properties = event
                .get("properties")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            properties
                .entry("sessionID".to_string())
                .or_insert(json!(session_id));
            properties
                .entry("messageID".to_string())
                .or_insert(json!(message_id));
            properties
                .entry("workspaceRoot".to_string())
                .or_insert(json!(workspace_root));
            properties
                .entry("effectiveCwd".to_string())
                .or_insert(json!(effective_cwd));
            bus.publish(EngineEvent::new(event_type, Value::Object(properties)));
        }
    }
}

fn apply_todo_updates_from_args(current: Vec<Value>, args: &Value) -> Option<Vec<Value>> {
    let obj = args.as_object()?;
    let mut todos = current;
    let mut changed = false;

    if let Some(items) = obj.get("todos").and_then(|v| v.as_array()) {
        for item in items {
            let Some(item_obj) = item.as_object() else {
                continue;
            };
            let status = item_obj
                .get("status")
                .and_then(|v| v.as_str())
                .map(normalize_todo_status);
            let target = item_obj
                .get("task_id")
                .or_else(|| item_obj.get("todo_id"))
                .or_else(|| item_obj.get("id"));

            if let (Some(status), Some(target)) = (status, target) {
                changed |= apply_single_todo_status_update(&mut todos, target, &status);
            }
        }
    }

    let status = obj
        .get("status")
        .and_then(|v| v.as_str())
        .map(normalize_todo_status);
    let target = obj
        .get("task_id")
        .or_else(|| obj.get("todo_id"))
        .or_else(|| obj.get("id"));
    if let (Some(status), Some(target)) = (status, target) {
        changed |= apply_single_todo_status_update(&mut todos, target, &status);
    }

    if changed {
        Some(todos)
    } else {
        None
    }
}

fn apply_single_todo_status_update(todos: &mut [Value], target: &Value, status: &str) -> bool {
    let idx_from_value = match target {
        Value::Number(n) => n.as_u64().map(|v| v.saturating_sub(1) as usize),
        Value::String(s) => {
            let trimmed = s.trim();
            trimmed
                .parse::<usize>()
                .ok()
                .map(|v| v.saturating_sub(1))
                .or_else(|| {
                    let digits = trimmed
                        .chars()
                        .rev()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>();
                    digits.parse::<usize>().ok().map(|v| v.saturating_sub(1))
                })
        }
        _ => None,
    };

    if let Some(idx) = idx_from_value {
        if idx < todos.len() {
            if let Some(obj) = todos[idx].as_object_mut() {
                obj.insert("status".to_string(), Value::String(status.to_string()));
                return true;
            }
        }
    }

    let id_target = target.as_str().map(|s| s.trim()).filter(|s| !s.is_empty());
    if let Some(id_target) = id_target {
        for todo in todos.iter_mut() {
            if let Some(obj) = todo.as_object_mut() {
                if obj.get("id").and_then(|v| v.as_str()) == Some(id_target) {
                    obj.insert("status".to_string(), Value::String(status.to_string()));
                    return true;
                }
            }
        }
    }

    false
}

fn normalize_todo_status(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "in_progress" | "inprogress" | "running" | "working" => "in_progress".to_string(),
        "done" | "complete" | "completed" => "completed".to_string(),
        "cancelled" | "canceled" | "aborted" | "skipped" => "cancelled".to_string(),
        "open" | "todo" | "pending" => "pending".to_string(),
        other => other.to_string(),
    }
}

fn compact_chat_history(
    messages: Vec<ChatMessage>,
    profile: ChatHistoryProfile,
) -> Vec<ChatMessage> {
    let (max_context_chars, keep_recent_messages) = match profile {
        ChatHistoryProfile::Full => (usize::MAX, usize::MAX),
        ChatHistoryProfile::Standard => (80_000usize, 40usize),
        ChatHistoryProfile::Compact => (12_000usize, 12usize),
    };

    if messages.len() <= keep_recent_messages {
        let total_chars = messages.iter().map(|m| m.content.len()).sum::<usize>();
        if total_chars <= max_context_chars {
            return messages;
        }
    }

    let mut kept = messages;
    let mut dropped_count = 0usize;
    let mut total_chars = kept.iter().map(|m| m.content.len()).sum::<usize>();

    while kept.len() > keep_recent_messages || total_chars > max_context_chars {
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
                attachments: Vec::new(),
            },
        );
    }
    kept
}
