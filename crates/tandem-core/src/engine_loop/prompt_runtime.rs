use uuid::Uuid;

use serde_json::{json, Value};
use tandem_providers::{ChatAttachment, ChatMessage};
use tandem_wire::WireMessagePart;

use crate::{EventBus, Storage};
use tandem_types::{EngineEvent, MessagePart, MessagePartInput};

use super::{extract_todo_candidates_from_text, tool_result_keep_recent, truncate_text};

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

impl ChatHistoryProfile {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            ChatHistoryProfile::Full => "full",
            ChatHistoryProfile::Standard => "standard",
            ChatHistoryProfile::Compact => "compact",
        }
    }
}

/// Provider-facing history projection plus accounting for what the
/// compaction step removed from the raw stored history.
#[derive(Debug)]
pub(super) struct LoadedChatHistory {
    pub(super) messages: Vec<ChatMessage>,
    pub(super) dropped_messages: usize,
    pub(super) dropped_chars: usize,
    pub(super) pinned_messages: usize,
    pub(super) compacted_tool_results: usize,
    pub(super) compacted_tool_result_chars: usize,
    pub(super) demoted_tool_invocations: usize,
    pub(super) demoted_tool_invocation_chars: usize,
}

impl LoadedChatHistory {
    fn from_messages(messages: Vec<ChatMessage>) -> Self {
        LoadedChatHistory {
            messages,
            dropped_messages: 0,
            dropped_chars: 0,
            pinned_messages: 0,
            compacted_tool_results: 0,
            compacted_tool_result_chars: 0,
            demoted_tool_invocations: 0,
            demoted_tool_invocation_chars: 0,
        }
    }
}

/// Provider-facing chat message paired with handles back to the raw stored
/// message it was projected from, so compaction can cite retrievable sources.
pub(super) struct SourcedChatMessage {
    pub(super) message: ChatMessage,
    pub(super) source_id: Option<String>,
    pub(super) source_index: usize,
}

/// Accounting for provider-history-only tool result compaction. Raw stored
/// tool results are never mutated; this tracks how much smaller the
/// projection is than the raw data.
#[derive(Debug, Default)]
struct ToolResultCompactionStats {
    compacted: usize,
    chars_saved: usize,
    demoted: usize,
    demoted_chars_saved: usize,
}

pub(super) async fn load_chat_history(
    storage: std::sync::Arc<Storage>,
    session_id: &str,
    profile: ChatHistoryProfile,
) -> LoadedChatHistory {
    let Some(session) = storage.get_session(session_id).await else {
        return LoadedChatHistory::from_messages(Vec::new());
    };
    let mut tool_compaction = ToolResultCompactionStats::default();
    // Recency policy for tool invocations: only the most recent
    // `tool_result_keep_recent()` invocations keep their full (compacted)
    // projection; everything older is demoted to a one-line summary with
    // provenance handles. Without this, every historical tool result and its
    // uncapped args are re-sent to the provider on every iteration for the
    // life of the session, and that accumulation dominates `historyChars` in
    // the context.budget.final telemetry for long tool-heavy sessions.
    let total_tool_invocations = session
        .messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .filter(|part| matches!(part, MessagePart::ToolInvocation { .. }))
        .count();
    let stale_cutoff = total_tool_invocations.saturating_sub(tool_result_keep_recent());
    let mut tool_invocation_ordinal = 0usize;
    let sourced = session
        .messages
        .into_iter()
        .enumerate()
        .map(|(source_index, m)| {
            let role = format!("{:?}", m.role).to_lowercase();
            let source_id = m.id.clone();
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
                    } => {
                        let stale = tool_invocation_ordinal < stale_cutoff;
                        tool_invocation_ordinal += 1;
                        if stale {
                            demote_stale_tool_invocation_for_history(
                                &tool,
                                &args,
                                result.as_ref(),
                                error.as_deref(),
                                &source_id,
                                &mut tool_compaction,
                            )
                        } else {
                            summarize_tool_invocation_for_history(
                                &tool,
                                &args,
                                result.as_ref(),
                                error.as_deref(),
                                &mut tool_compaction,
                            )
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            SourcedChatMessage {
                message: ChatMessage {
                    role,
                    content,
                    attachments: Vec::new(),
                },
                source_id: Some(source_id),
                source_index,
            }
        })
        .collect::<Vec<_>>();
    let mut loaded = compact_chat_history_sourced(sourced, profile);
    loaded.compacted_tool_results = tool_compaction.compacted;
    loaded.compacted_tool_result_chars = tool_compaction.chars_saved;
    loaded.demoted_tool_invocations = tool_compaction.demoted;
    loaded.demoted_tool_invocation_chars = tool_compaction.demoted_chars_saved;
    loaded
}

const STALE_TOOL_ARGS_PREVIEW_CHARS: usize = 200;
const STALE_TOOL_ERROR_PREVIEW_CHARS: usize = 200;

/// Concise projection for a tool invocation older than the keep-recent
/// window: what was called, whether it succeeded, and provenance handles
/// (source message id + original tool/args) so the model can re-retrieve the
/// data — by re-running the tool or citing the session message — instead of
/// carrying the full payload in every subsequent provider request. The raw
/// stored record is never mutated.
fn demote_stale_tool_invocation_for_history(
    tool: &str,
    args: &Value,
    result: Option<&Value>,
    error: Option<&str>,
    source_message_id: &str,
    stats: &mut ToolResultCompactionStats,
) -> String {
    let args_serialized = if args.is_null() {
        String::new()
    } else {
        args.to_string()
    };
    let result_serialized = result
        .filter(|value| !value.is_null())
        .map(|value| value.to_string())
        .unwrap_or_default();
    let raw_len = args_serialized.len() + result_serialized.len();

    let mut segments = vec![format!("Tool {tool}")];
    if !args_serialized.is_empty() {
        segments.push(format!(
            "args≈{}",
            truncate_text(&args_serialized, STALE_TOOL_ARGS_PREVIEW_CHARS)
        ));
    }
    // Failures stay visible even when stale: knowing an earlier attempt
    // failed (and why) is load-bearing context that is cheap to keep.
    if let Some(error) = error.map(str::trim).filter(|value| !value.is_empty()) {
        segments.push(format!(
            "error={}",
            truncate_text(error, STALE_TOOL_ERROR_PREVIEW_CHARS)
        ));
    } else if !result_serialized.is_empty() {
        segments.push("status=ok".to_string());
    }
    segments.push(format!(
        "result=[stale; {} chars demoted from provider history; full record in session message {}; re-run {} with the original arguments if this data is needed again]",
        result_serialized.len(),
        source_message_id,
        tool
    ));
    let line = segments.join(" ");

    stats.demoted += 1;
    stats.demoted_chars_saved += raw_len.saturating_sub(line.len());
    line
}

fn summarize_tool_invocation_for_history(
    tool: &str,
    args: &Value,
    result: Option<&Value>,
    error: Option<&str>,
    stats: &mut ToolResultCompactionStats,
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
        let compacted = compact_tool_result_for_history(tool, result, stats);
        segments.push(format!("result={compacted}"));
    }
    if segments.len() == 1 {
        segments.push("result={}".to_string());
    }
    segments.join(" ")
}

/// Slack above the head+tail budget before output compaction kicks in, so
/// marginally-over outputs are not replaced with a same-sized marker.
const TOOL_OUTPUT_COMPACTION_SLACK: usize = 512;
/// Serialized-size ceiling for tool results whose shape has no known
/// compaction path; larger results are replaced with a capped preview.
const UNKNOWN_TOOL_RESULT_HISTORY_CAP: usize = 6_000;
const UNKNOWN_TOOL_RESULT_PREVIEW_CHARS: usize = 2_000;

/// (head, tail) char budget for a tool's `output` field in provider-facing
/// history. Tail is preserved for shell-style tools because exit status and
/// error summaries land at the end of the stream.
fn tool_output_history_budget(tool_key: &str) -> (usize, usize) {
    match tool_key {
        "bash" | "shell" | "powershell" | "cmd" => (1_600, 800),
        "read" | "write" | "edit" | "apply_patch" => (2_000, 400),
        "grep" | "glob" | "search" | "codebase_search" | "ls" => (2_000, 0),
        key if key.starts_with("web") || key.contains("fetch") || key.contains("search") => {
            (2_000, 0)
        }
        _ => (2_400, 600),
    }
}

fn compact_tool_result_for_history(
    tool: &str,
    result: &Value,
    stats: &mut ToolResultCompactionStats,
) -> Value {
    let tool_key = tool.trim().to_ascii_lowercase();
    let raw_len = result.to_string().len();
    if tool_key == "mcp_list" {
        let compacted = compact_mcp_list_result_for_history(result);
        record_tool_result_compaction(stats, raw_len, &compacted);
        return compacted;
    }
    let mut projected = result.clone();
    if let Some(output) = result.get("output").and_then(Value::as_str) {
        let (head, tail) = tool_output_history_budget(&tool_key);
        if output.len() > head + tail + TOOL_OUTPUT_COMPACTION_SLACK {
            if let Some(obj) = projected.as_object_mut() {
                obj.insert(
                    "output".to_string(),
                    Value::String(compact_output_head_tail(output, head, tail)),
                );
            }
        }
    }
    // Capped fallback: result shapes that are still oversized after any known
    // output compaction (huge metadata, unknown nested structures) are
    // replaced with a bounded preview rather than sent verbatim.
    let projected_serialized = projected.to_string();
    if projected_serialized.len() > UNKNOWN_TOOL_RESULT_HISTORY_CAP {
        let preview = truncate_text(&projected_serialized, UNKNOWN_TOOL_RESULT_PREVIEW_CHARS);
        projected = json!({
            "summary": format!("{tool} result compacted for chat history"),
            "preview": preview,
            "omittedChars": projected_serialized.len().saturating_sub(UNKNOWN_TOOL_RESULT_PREVIEW_CHARS),
        });
    }
    record_tool_result_compaction(stats, raw_len, &projected);
    projected
}

fn record_tool_result_compaction(
    stats: &mut ToolResultCompactionStats,
    raw_len: usize,
    compacted: &Value,
) {
    let compacted_len = compacted.to_string().len();
    if compacted_len < raw_len {
        stats.compacted += 1;
        stats.chars_saved += raw_len - compacted_len;
    }
}

fn compact_output_head_tail(output: &str, head: usize, tail: usize) -> String {
    let head_end = char_boundary_at_most(output, head);
    let tail_start = char_boundary_at_least(output, output.len().saturating_sub(tail));
    let omitted = tail_start.saturating_sub(head_end);
    let tail_part = if tail_start > head_end {
        &output[tail_start..]
    } else {
        ""
    };
    format!(
        "{}\n…[tool output compacted for provider history: omitted {} chars]…\n{}",
        &output[..head_end],
        omitted,
        tail_part
    )
}

fn char_boundary_at_most(input: &str, index: usize) -> usize {
    if index >= input.len() {
        return input.len();
    }
    let mut boundary = index;
    while boundary > 0 && !input.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn char_boundary_at_least(input: &str, index: usize) -> usize {
    if index >= input.len() {
        return input.len();
    }
    let mut boundary = index;
    while boundary < input.len() && !input.is_char_boundary(boundary) {
        boundary += 1;
    }
    boundary
}

fn compact_mcp_list_result_for_history(result: &Value) -> Value {
    const MAX_TOOLS: usize = 40;
    let mut tool_names = Vec::new();
    collect_string_array(result, "registered_tools", &mut tool_names);
    collect_string_array(result, "remote_tools", &mut tool_names);
    tool_names.sort();
    tool_names.dedup();

    let mut connected_server_names = Vec::new();
    collect_string_array(
        result,
        "connected_server_names",
        &mut connected_server_names,
    );
    connected_server_names.sort();
    connected_server_names.dedup();

    let total_registered_tools = result
        .get("registered_tool_count")
        .and_then(Value::as_u64)
        .or_else(|| result.get("registeredToolCount").and_then(Value::as_u64))
        .unwrap_or(tool_names.len() as u64);
    let total_remote_tools = result
        .get("remote_tool_count")
        .and_then(Value::as_u64)
        .or_else(|| result.get("remoteToolCount").and_then(Value::as_u64))
        .unwrap_or(total_registered_tools);

    let truncated = tool_names.len() > MAX_TOOLS;
    tool_names.truncate(MAX_TOOLS);

    json!({
        "summary": "mcp_list result compacted for chat history",
        "connected_server_names": connected_server_names,
        "registered_tool_count": total_registered_tools,
        "remote_tool_count": total_remote_tools,
        "registered_tools_sample": tool_names,
        "truncated": truncated,
    })
}

fn collect_string_array(value: &Value, key: &str, out: &mut Vec<String>) {
    if let Some(rows) = value.get(key).and_then(Value::as_array) {
        out.extend(
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string),
        );
    }
    if let Some(servers) = value.get("servers").and_then(Value::as_array) {
        for server in servers {
            collect_string_array(server, key, out);
        }
    }
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

pub(super) fn apply_todo_updates_from_args(
    current: Vec<Value>,
    args: &Value,
) -> Option<Vec<Value>> {
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

/// Char cap applied to each guardrail/decision message pulled forward from
/// the compacted prefix, so pinning cannot reinflate the projection.
const PINNED_MESSAGE_CHAR_CAP: usize = 600;
const MAX_PINNED_MESSAGES: usize = 6;

/// True when a message must survive history compaction: system/runtime
/// guardrails, plus approval/rejection/decision boundaries and pending
/// questions that later turns may depend on.
fn is_pinned_history_message(role: &str, content: &str) -> bool {
    if role == "system" {
        return true;
    }
    let lowered = content.to_lowercase();
    const DECISION_MARKERS: [&str; 10] = [
        "approval granted",
        "approval denied",
        "approved:",
        "rejected:",
        "permission granted",
        "permission denied",
        "pending question",
        "unresolved decision",
        "task goal",
        "workflow state",
    ];
    DECISION_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

pub(super) fn compact_chat_history(
    messages: Vec<ChatMessage>,
    profile: ChatHistoryProfile,
) -> LoadedChatHistory {
    let sourced = messages
        .into_iter()
        .enumerate()
        .map(|(source_index, message)| SourcedChatMessage {
            message,
            source_id: None,
            source_index,
        })
        .collect();
    compact_chat_history_sourced(sourced, profile)
}

/// Projects raw history into a bounded provider-facing view. Old messages
/// are dropped from the front, but the omission note carries retrievable
/// provenance handles (source message range and IDs), and guardrail/decision
/// messages from the dropped prefix are pinned forward in truncated form.
/// Raw stored history is never mutated.
pub(super) fn compact_chat_history_sourced(
    sourced: Vec<SourcedChatMessage>,
    profile: ChatHistoryProfile,
) -> LoadedChatHistory {
    let (max_context_chars, keep_recent_messages) = match profile {
        ChatHistoryProfile::Full => (usize::MAX, usize::MAX),
        ChatHistoryProfile::Standard => (80_000usize, 40usize),
        ChatHistoryProfile::Compact => (12_000usize, 12usize),
    };

    if sourced.len() <= keep_recent_messages {
        let total_chars = sourced
            .iter()
            .map(|m| m.message.content.len())
            .sum::<usize>();
        if total_chars <= max_context_chars {
            return LoadedChatHistory::from_messages(
                sourced.into_iter().map(|m| m.message).collect(),
            );
        }
    }

    let mut kept = sourced;
    let mut dropped_count = 0usize;
    let mut dropped_chars = 0usize;
    let mut pinned: Vec<SourcedChatMessage> = Vec::new();
    let mut prefix_first_index: Option<usize> = None;
    let mut prefix_last_index: Option<usize> = None;
    let mut prefix_first_id: Option<String> = None;
    let mut prefix_last_id: Option<String> = None;
    let mut total_chars = kept.iter().map(|m| m.message.content.len()).sum::<usize>();

    while kept.len() > keep_recent_messages || total_chars > max_context_chars {
        if kept.is_empty() {
            break;
        }
        let removed = kept.remove(0);
        total_chars = total_chars.saturating_sub(removed.message.content.len());
        if prefix_first_index.is_none() {
            prefix_first_index = Some(removed.source_index);
            prefix_first_id = removed.source_id.clone();
        }
        prefix_last_index = Some(removed.source_index);
        prefix_last_id = removed.source_id.clone();
        if is_pinned_history_message(&removed.message.role, &removed.message.content) {
            pinned.push(removed);
            continue;
        }
        dropped_chars = dropped_chars.saturating_add(removed.message.content.len());
        dropped_count += 1;
    }

    // Keep the most recent pinned entries; older overflow is folded back
    // into the dropped accounting (it stays retrievable via the note).
    if pinned.len() > MAX_PINNED_MESSAGES {
        for overflow in pinned.drain(..pinned.len() - MAX_PINNED_MESSAGES) {
            dropped_chars = dropped_chars.saturating_add(overflow.message.content.len());
            dropped_count += 1;
        }
    }

    let pinned_count = pinned.len();
    let mut projected = Vec::with_capacity(kept.len() + pinned_count + 1);
    if dropped_count > 0 || pinned_count > 0 {
        let range = match (prefix_first_index, prefix_last_index) {
            (Some(first), Some(last)) => format!("; source messages {first}-{last}"),
            _ => String::new(),
        };
        let ids = match (prefix_first_id.as_deref(), prefix_last_id.as_deref()) {
            (Some(first), Some(last)) => format!(" (ids {first}..{last})"),
            _ => String::new(),
        };
        let pinned_note = if pinned_count > 0 {
            format!("; {pinned_count} guardrail/decision messages pinned below")
        } else {
            String::new()
        };
        projected.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "[history compacted: omitted {dropped_count} older messages ({dropped_chars} chars){range}{ids}{pinned_note}; full transcript remains in stored session history]"
            ),
            attachments: Vec::new(),
        });
    }
    for pin in pinned {
        let source = match pin.source_id.as_deref() {
            Some(id) => format!("source message {} (id {id})", pin.source_index),
            None => format!("source message {}", pin.source_index),
        };
        projected.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "[pinned from compacted history; {source}] {}",
                truncate_text(&pin.message.content, PINNED_MESSAGE_CHAR_CAP)
            ),
            attachments: Vec::new(),
        });
    }
    projected.extend(kept.into_iter().map(|m| m.message));

    LoadedChatHistory {
        messages: projected,
        dropped_messages: dropped_count,
        dropped_chars,
        pinned_messages: pinned_count,
        compacted_tool_results: 0,
        compacted_tool_result_chars: 0,
        demoted_tool_invocations: 0,
        demoted_tool_invocation_chars: 0,
    }
}

/// Approximate char contribution of runtime attachments. Data URLs carry the
/// full encoded payload in the URL, so URL length is the dominant cost.
pub(super) fn runtime_attachment_chars(attachments: &[ChatAttachment]) -> usize {
    attachments
        .iter()
        .map(|attachment| match attachment {
            ChatAttachment::ImageUrl { url } => url.len(),
        })
        .sum()
}
