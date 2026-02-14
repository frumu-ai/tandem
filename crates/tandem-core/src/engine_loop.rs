use chrono::Utc;
use futures::StreamExt;
use serde_json::{json, Map, Number, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tandem_providers::{ChatMessage, ProviderRegistry, StreamChunk};
use tandem_tools::{validate_tool_schemas, ToolRegistry};
use tandem_types::{
    EngineEvent, Message, MessagePart, MessagePartInput, MessageRole, SendMessageRequest,
};
use tandem_wire::WireMessagePart;
use tokio_util::sync::CancellationToken;
use tracing::Level;

use crate::{
    AgentDefinition, AgentRegistry, CancellationRegistry, EventBus, PermissionAction,
    PermissionManager, PluginRegistry, Storage,
};
use tokio::sync::RwLock;

#[derive(Default)]
struct StreamedToolCall {
    name: String,
    args: String,
}

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
    workspace_overrides: std::sync::Arc<RwLock<HashMap<String, u64>>>,
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
            workspace_overrides: std::sync::Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn grant_workspace_override_for_session(
        &self,
        session_id: &str,
        ttl_seconds: u64,
    ) -> u64 {
        let expires_at = chrono::Utc::now()
            .timestamp_millis()
            .max(0)
            .saturating_add((ttl_seconds as i64).saturating_mul(1000))
            as u64;
        self.workspace_overrides
            .write()
            .await
            .insert(session_id.to_string(), expires_at);
        expires_at
    }

    pub async fn run_prompt_async(
        &self,
        session_id: String,
        req: SendMessageRequest,
    ) -> anyhow::Result<()> {
        self.run_prompt_async_with_context(session_id, req, None)
            .await
    }

    pub async fn run_prompt_async_with_context(
        &self,
        session_id: String,
        req: SendMessageRequest,
        correlation_id: Option<String>,
    ) -> anyhow::Result<()> {
        let session_provider = self
            .storage
            .get_session(&session_id)
            .await
            .and_then(|s| s.provider);
        let provider_hint = req
            .model
            .as_ref()
            .map(|m| m.provider_id.clone())
            .or(session_provider);
        let correlation_ref = correlation_id.as_deref();
        let model_id = req.model.as_ref().map(|m| m.model_id.as_str());
        let cancel = self.cancellations.create(&session_id).await;
        emit_event(
            Level::INFO,
            ProcessKind::Engine,
            ObservabilityEvent {
                event: "provider.call.start",
                component: "engine.loop",
                correlation_id: correlation_ref,
                session_id: Some(&session_id),
                run_id: None,
                message_id: None,
                provider_id: provider_hint.as_deref(),
                model_id,
                status: Some("start"),
                error_code: None,
                detail: Some("run_prompt_async dispatch"),
            },
        );
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
        let mut user_message_id = self
            .find_recent_matching_user_message_id(&session_id, &text)
            .await;
        if user_message_id.is_none() {
            let user_message = Message::new(
                MessageRole::User,
                vec![MessagePart::Text { text: text.clone() }],
            );
            let created_message_id = user_message.id.clone();
            self.storage
                .append_message(&session_id, user_message)
                .await?;

            let user_part = WireMessagePart::text(&session_id, &created_message_id, text.clone());
            self.event_bus.publish(EngineEvent::new(
                "message.part.updated",
                json!({
                    "part": user_part,
                    "delta": text,
                    "agent": active_agent.name
                }),
            ));
            user_message_id = Some(created_message_id);
        }
        let user_message_id = user_message_id.unwrap_or_else(|| "unknown".to_string());

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
            let mut last_tool_outputs: Vec<String> = Vec::new();

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
                let tool_schemas = self.tools.list().await;
                if let Err(validation_err) = validate_tool_schemas(&tool_schemas) {
                    let detail = validation_err.to_string();
                    emit_event(
                        Level::ERROR,
                        ProcessKind::Engine,
                        ObservabilityEvent {
                            event: "provider.call.error",
                            component: "engine.loop",
                            correlation_id: correlation_ref,
                            session_id: Some(&session_id),
                            run_id: None,
                            message_id: Some(&user_message_id),
                            provider_id: provider_hint.as_deref(),
                            model_id,
                            status: Some("failed"),
                            error_code: Some("TOOL_SCHEMA_INVALID"),
                            detail: Some(&detail),
                        },
                    );
                    anyhow::bail!("{detail}");
                }
                let stream = self
                    .providers
                    .stream_for_provider(
                        provider_hint.as_deref(),
                        messages,
                        Some(tool_schemas),
                        cancel.clone(),
                    )
                    .await
                    .map_err(|err| {
                        let error_text = err.to_string();
                        let error_code = provider_error_code(&error_text);
                        let detail = truncate_text(&error_text, 500);
                        emit_event(
                            Level::ERROR,
                            ProcessKind::Engine,
                            ObservabilityEvent {
                                event: "provider.call.error",
                                component: "engine.loop",
                                correlation_id: correlation_ref,
                                session_id: Some(&session_id),
                                run_id: None,
                                message_id: Some(&user_message_id),
                                provider_id: provider_hint.as_deref(),
                                model_id,
                                status: Some("failed"),
                                error_code: Some(error_code),
                                detail: Some(&detail),
                            },
                        );
                        err
                    })?;
                tokio::pin!(stream);
                completion.clear();
                let mut streamed_tool_calls: HashMap<String, StreamedToolCall> = HashMap::new();
                while let Some(chunk) = stream.next().await {
                    let chunk = match chunk {
                        Ok(chunk) => chunk,
                        Err(err) => {
                            let error_text = err.to_string();
                            let error_code = provider_error_code(&error_text);
                            let detail = truncate_text(&error_text, 500);
                            emit_event(
                                Level::ERROR,
                                ProcessKind::Engine,
                                ObservabilityEvent {
                                    event: "provider.call.error",
                                    component: "engine.loop",
                                    correlation_id: correlation_ref,
                                    session_id: Some(&session_id),
                                    run_id: None,
                                    message_id: Some(&user_message_id),
                                    provider_id: provider_hint.as_deref(),
                                    model_id,
                                    status: Some("failed"),
                                    error_code: Some(error_code),
                                    detail: Some(&detail),
                                },
                            );
                            return Err(anyhow::anyhow!(
                                "provider stream chunk error: {error_text}"
                            ));
                        }
                    };
                    match chunk {
                        StreamChunk::TextDelta(delta) => {
                            if completion.is_empty() {
                                emit_event(
                                    Level::INFO,
                                    ProcessKind::Engine,
                                    ObservabilityEvent {
                                        event: "provider.call.first_byte",
                                        component: "engine.loop",
                                        correlation_id: correlation_ref,
                                        session_id: Some(&session_id),
                                        run_id: None,
                                        message_id: Some(&user_message_id),
                                        provider_id: provider_hint.as_deref(),
                                        model_id,
                                        status: Some("streaming"),
                                        error_code: None,
                                        detail: Some("first text delta"),
                                    },
                                );
                            }
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
                        StreamChunk::ToolCallStart { id, name } => {
                            let entry = streamed_tool_calls.entry(id).or_default();
                            if entry.name.is_empty() {
                                entry.name = name;
                            }
                        }
                        StreamChunk::ToolCallDelta { id, args_delta } => {
                            let entry = streamed_tool_calls.entry(id).or_default();
                            entry.args.push_str(&args_delta);
                        }
                        StreamChunk::ToolCallEnd { id: _ } => {}
                    }
                    if cancel.is_cancelled() {
                        break;
                    }
                }

                let mut tool_calls = streamed_tool_calls
                    .into_values()
                    .filter_map(|call| {
                        if call.name.trim().is_empty() {
                            return None;
                        }
                        let parsed_args = serde_json::from_str::<Value>(call.args.trim())
                            .unwrap_or_else(|_| json!({}));
                        Some((normalize_tool_name(&call.name), parsed_args))
                    })
                    .collect::<Vec<_>>();
                if tool_calls.is_empty() {
                    tool_calls = parse_tool_invocations_from_response(&completion);
                }
                if !tool_calls.is_empty() {
                    let mut outputs = Vec::new();
                    for (tool, args) in tool_calls {
                        if !agent_can_use_tool(&active_agent, &tool) {
                            continue;
                        }
                        if let Some(output) = self
                            .execute_tool_with_permission(
                                &session_id,
                                &user_message_id,
                                tool,
                                args,
                                cancel.clone(),
                            )
                            .await?
                        {
                            outputs.push(output);
                        }
                    }
                    if !outputs.is_empty() {
                        last_tool_outputs = outputs.clone();
                        followup_context = Some(format!("{}\nContinue.", outputs.join("\n\n")));
                        continue;
                    }
                }

                break;
            }
            if completion.trim().is_empty() && !last_tool_outputs.is_empty() {
                let preview = last_tool_outputs
                    .iter()
                    .take(3)
                    .map(|o| truncate_text(o, 240))
                    .collect::<Vec<_>>()
                    .join("\n");
                completion = format!(
                    "I completed project analysis steps using tools, but the model returned no final narrative text.\n\nTool result summary:\n{}",
                    preview
                );
            }
            truncate_text(&completion, 16_000)
        };
        emit_event(
            Level::INFO,
            ProcessKind::Engine,
            ObservabilityEvent {
                event: "provider.call.finish",
                component: "engine.loop",
                correlation_id: correlation_ref,
                session_id: Some(&session_id),
                run_id: None,
                message_id: Some(&user_message_id),
                provider_id: provider_hint.as_deref(),
                model_id,
                status: Some("ok"),
                error_code: None,
                detail: Some("provider stream complete"),
            },
        );
        if active_agent.name.eq_ignore_ascii_case("plan") {
            emit_plan_todo_fallback(
                self.storage.clone(),
                &self.event_bus,
                &session_id,
                &user_message_id,
                &completion,
            )
            .await;
        }
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
        let tool = normalize_tool_name(&tool);
        if let Some(violation) = self
            .workspace_sandbox_violation(session_id, &tool, &args)
            .await
        {
            let mut blocked_part =
                WireMessagePart::tool_result(session_id, message_id, tool.clone(), json!(null));
            blocked_part.state = Some("failed".to_string());
            blocked_part.error = Some(violation.clone());
            self.event_bus.publish(EngineEvent::new(
                "message.part.updated",
                json!({"part": blocked_part}),
            ));
            return Ok(Some(violation));
        }
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
        let args_for_side_events = args.clone();
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
            &args_for_side_events,
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

    async fn find_recent_matching_user_message_id(
        &self,
        session_id: &str,
        text: &str,
    ) -> Option<String> {
        let session = self.storage.get_session(session_id).await?;
        let last = session.messages.last()?;
        if !matches!(last.role, MessageRole::User) {
            return None;
        }
        let age_ms = (Utc::now() - last.created_at).num_milliseconds().max(0) as u64;
        if age_ms > 10_000 {
            return None;
        }
        let last_text = last
            .parts
            .iter()
            .filter_map(|part| match part {
                MessagePart::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if last_text == text {
            return Some(last.id.clone());
        }
        None
    }

    async fn workspace_sandbox_violation(
        &self,
        session_id: &str,
        tool: &str,
        args: &Value,
    ) -> Option<String> {
        if self.workspace_override_active(session_id).await {
            return None;
        }
        let session = self.storage.get_session(session_id).await?;
        let workspace = session
            .workspace_root
            .or_else(|| crate::normalize_workspace_path(&session.directory))?;
        let workspace_path = PathBuf::from(&workspace);
        let candidate_paths = extract_tool_candidate_paths(tool, args);
        if candidate_paths.is_empty() {
            return None;
        }
        let outside = candidate_paths
            .iter()
            .find(|path| !crate::is_within_workspace_root(Path::new(path), &workspace_path))?;
        Some(format!(
            "Sandbox blocked `{tool}` path `{outside}` (workspace root: `{workspace}`)"
        ))
    }

    async fn workspace_override_active(&self, session_id: &str) -> bool {
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let mut overrides = self.workspace_overrides.write().await;
        overrides.retain(|_, expires_at| *expires_at > now);
        overrides
            .get(session_id)
            .map(|expires_at| *expires_at > now)
            .unwrap_or(false)
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

fn provider_error_code(error_text: &str) -> &'static str {
    let lower = error_text.to_lowercase();
    if lower.contains("invalid_function_parameters")
        || lower.contains("array schema missing items")
        || lower.contains("tool schema")
    {
        return "TOOL_SCHEMA_INVALID";
    }
    if lower.contains("rate limit") || lower.contains("too many requests") || lower.contains("429")
    {
        return "RATE_LIMIT_EXCEEDED";
    }
    if lower.contains("context length")
        || lower.contains("max tokens")
        || lower.contains("token limit")
    {
        return "CONTEXT_LENGTH_EXCEEDED";
    }
    if lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("401")
        || lower.contains("403")
    {
        return "AUTHENTICATION_ERROR";
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return "TIMEOUT";
    }
    if lower.contains("server error")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        return "PROVIDER_SERVER_ERROR";
    }
    "PROVIDER_REQUEST_FAILED"
}

fn normalize_tool_name(name: &str) -> String {
    match name.trim().to_lowercase().replace('-', "_").as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        other => other.to_string(),
    }
}

fn extract_tool_candidate_paths(tool: &str, args: &Value) -> Vec<String> {
    let Some(obj) = args.as_object() else {
        return Vec::new();
    };
    let keys: &[&str] = match tool {
        "read" | "write" | "edit" | "grep" | "codesearch" => &["path", "filePath", "cwd"],
        "glob" => &["pattern"],
        "lsp" => &["filePath", "path"],
        "bash" => &["cwd"],
        "apply_patch" => &[],
        _ => &["path", "cwd"],
    };
    keys.iter()
        .filter_map(|key| obj.get(*key))
        .filter_map(|value| value.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(ToString::to_string)
        .collect()
}

fn agent_can_use_tool(agent: &AgentDefinition, tool_name: &str) -> bool {
    let target = normalize_tool_name(tool_name);
    match agent.tools.as_ref() {
        None => true,
        Some(list) => list.iter().any(|t| normalize_tool_name(t) == target),
    }
}

fn parse_tool_invocation(input: &str) -> Option<(String, serde_json::Value)> {
    let raw = input.trim();
    if !raw.starts_with("/tool ") {
        return None;
    }
    let rest = raw.trim_start_matches("/tool ").trim();
    let mut split = rest.splitn(2, ' ');
    let tool = normalize_tool_name(split.next()?.trim());
    let args = split
        .next()
        .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
        .unwrap_or_else(|| json!({}));
    Some((tool, args))
}

fn parse_tool_invocations_from_response(input: &str) -> Vec<(String, serde_json::Value)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(found) = extract_tool_call_from_value(&parsed) {
            return vec![found];
        }
    }

    if let Some(block) = extract_first_json_object(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&block) {
            if let Some(found) = extract_tool_call_from_value(&parsed) {
                return vec![found];
            }
        }
    }

    parse_function_style_tool_calls(trimmed)
}

#[cfg(test)]
fn parse_tool_invocation_from_response(input: &str) -> Option<(String, serde_json::Value)> {
    parse_tool_invocations_from_response(input)
        .into_iter()
        .next()
}

fn parse_function_style_tool_calls(input: &str) -> Vec<(String, Value)> {
    let mut calls = Vec::new();
    let lower = input.to_lowercase();
    let names = [
        "todo_write",
        "todowrite",
        "update_todo_list",
        "update_todos",
    ];
    let mut cursor = 0usize;

    while cursor < lower.len() {
        let mut best: Option<(usize, &str)> = None;
        for name in names {
            let needle = format!("{name}(");
            if let Some(rel_idx) = lower[cursor..].find(&needle) {
                let idx = cursor + rel_idx;
                if best.as_ref().map_or(true, |(best_idx, _)| idx < *best_idx) {
                    best = Some((idx, name));
                }
            }
        }

        let Some((tool_start, tool_name)) = best else {
            break;
        };

        let open_paren = tool_start + tool_name.len();
        if let Some(close_paren) = find_matching_paren(input, open_paren) {
            if let Some(args_text) = input.get(open_paren + 1..close_paren) {
                let args = parse_function_style_args(args_text.trim());
                calls.push((normalize_tool_name(tool_name), Value::Object(args)));
            }
            cursor = close_paren.saturating_add(1);
        } else {
            cursor = tool_start.saturating_add(tool_name.len());
        }
    }

    calls
}

fn find_matching_paren(input: &str, open_paren: usize) -> Option<usize> {
    if input.as_bytes().get(open_paren).copied()? != b'(' {
        return None;
    }

    let mut depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for (offset, ch) in input.get(open_paren..)?.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && (in_single || in_double) {
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if in_single || in_double {
            continue;
        }

        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open_paren + offset);
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_function_style_args(input: &str) -> Map<String, Value> {
    let mut args = Map::new();
    if input.trim().is_empty() {
        return args;
    }

    let mut parts = Vec::<String>::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut depth_paren = 0usize;
    let mut depth_bracket = 0usize;
    let mut depth_brace = 0usize;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && (in_single || in_double) {
            current.push(ch);
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            current.push(ch);
            continue;
        }
        if in_single || in_double {
            current.push(ch);
            continue;
        }

        match ch {
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            '[' => depth_bracket += 1,
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            ',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_string());
                }
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    let tail = current.trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }

    for part in parts {
        let Some((raw_key, raw_value)) = part
            .split_once('=')
            .or_else(|| part.split_once(':'))
            .map(|(k, v)| (k.trim(), v.trim()))
        else {
            continue;
        };
        let key = raw_key.trim_matches(|c| c == '"' || c == '\'' || c == '`');
        if key.is_empty() {
            continue;
        }
        let value = parse_scalar_like_value(raw_value);
        args.insert(key.to_string(), value);
    }

    args
}

fn parse_scalar_like_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }

    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return Value::String(trimmed[1..trimmed.len().saturating_sub(1)].to_string());
    }

    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if trimmed.eq_ignore_ascii_case("null") {
        return Value::Null;
    }

    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return v;
    }
    if let Ok(v) = trimmed.parse::<i64>() {
        return Value::Number(Number::from(v));
    }
    if let Ok(v) = trimmed.parse::<f64>() {
        if let Some(n) = Number::from_f64(v) {
            return Value::Number(n);
        }
    }

    Value::String(trimmed.to_string())
}

fn extract_tool_call_from_value(value: &Value) -> Option<(String, Value)> {
    if let Some(obj) = value.as_object() {
        if let Some(tool) = obj.get("tool").and_then(|v| v.as_str()) {
            return Some((
                normalize_tool_name(tool),
                obj.get("args").cloned().unwrap_or_else(|| json!({})),
            ));
        }

        if let Some(tool) = obj.get("name").and_then(|v| v.as_str()) {
            let args = obj
                .get("args")
                .cloned()
                .or_else(|| obj.get("arguments").cloned())
                .unwrap_or_else(|| json!({}));
            let args = if let Some(raw) = args.as_str() {
                serde_json::from_str::<Value>(raw).unwrap_or_else(|_| json!({}))
            } else {
                args
            };
            return Some((normalize_tool_name(tool), args));
        }

        for key in [
            "tool_call",
            "toolCall",
            "call",
            "function_call",
            "functionCall",
        ] {
            if let Some(nested) = obj.get(key) {
                if let Some(found) = extract_tool_call_from_value(nested) {
                    return Some(found);
                }
            }
        }
    }

    if let Some(items) = value.as_array() {
        for item in items {
            if let Some(found) = extract_tool_call_from_value(item) {
                return Some(found);
            }
        }
    }

    None
}

fn extract_first_json_object(input: &str) -> Option<String> {
    let mut start = None;
    let mut depth = 0usize;
    for (idx, ch) in input.char_indices() {
        if ch == '{' {
            if start.is_none() {
                start = Some(idx);
            }
            depth += 1;
        } else if ch == '}' {
            if depth == 0 {
                continue;
            }
            depth -= 1;
            if depth == 0 {
                let begin = start?;
                let block = input.get(begin..=idx)?;
                return Some(block.to_string());
            }
        }
    }
    None
}

fn extract_todo_candidates_from_text(input: &str) -> Vec<Value> {
    let mut seen = HashSet::<String>::new();
    let mut todos = Vec::new();

    for raw_line in input.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("```") {
            continue;
        }
        if line.ends_with(':') {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("- [ ]")
            .or_else(|| line.strip_prefix("* [ ]"))
            .or_else(|| line.strip_prefix("- [x]"))
            .or_else(|| line.strip_prefix("* [x]"))
        {
            line = rest.trim();
        } else if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            line = rest.trim();
        } else {
            let bytes = line.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > 0 && i + 1 < bytes.len() && (bytes[i] == b'.' || bytes[i] == b')') {
                line = line[i + 1..].trim();
            }
        }

        let content = line.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '*');
        if content.len() < 5 || content.len() > 180 {
            continue;
        }
        let key = content.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        todos.push(json!({ "content": content }));
        if todos.len() >= 25 {
            break;
        }
    }

    todos
}

async fn emit_plan_todo_fallback(
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

    if storage.set_todos(session_id, todos).await.is_err() {
        let mut failed_part =
            WireMessagePart::tool_result(session_id, message_id, "todo_write", json!(null));
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
    args: &serde_json::Value,
    metadata: &serde_json::Value,
) {
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

    #[test]
    fn extracts_todos_from_checklist_and_numbered_lines() {
        let input = r#"
Plan:
- [ ] Audit current implementation
- [ ] Add planner fallback
1. Add regression test coverage
"#;
        let todos = extract_todo_candidates_from_text(input);
        assert_eq!(todos.len(), 3);
        assert_eq!(
            todos[0].get("content").and_then(|v| v.as_str()),
            Some("Audit current implementation")
        );
    }

    #[test]
    fn parses_wrapped_tool_call_from_markdown_response() {
        let input = r#"
Here is the tool call:
```json
{"tool_call":{"name":"todo_write","arguments":{"todos":[{"content":"a"}]}}}
```
"#;
        let parsed = parse_tool_invocation_from_response(input).expect("tool call");
        assert_eq!(parsed.0, "todo_write");
        assert!(parsed.1.get("todos").is_some());
    }

    #[test]
    fn parses_function_style_todowrite_call() {
        let input = r#"Status: Completed
Call: todowrite(task_id=2, status="completed")"#;
        let parsed = parse_tool_invocation_from_response(input).expect("function-style tool call");
        assert_eq!(parsed.0, "todo_write");
        assert_eq!(parsed.1.get("task_id").and_then(|v| v.as_i64()), Some(2));
        assert_eq!(
            parsed.1.get("status").and_then(|v| v.as_str()),
            Some("completed")
        );
    }

    #[test]
    fn parses_multiple_function_style_todowrite_calls() {
        let input = r#"
Call: todowrite(task_id=2, status="completed")
Call: todowrite(task_id=3, status="in_progress")
"#;
        let parsed = parse_tool_invocations_from_response(input);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "todo_write");
        assert_eq!(parsed[0].1.get("task_id").and_then(|v| v.as_i64()), Some(2));
        assert_eq!(
            parsed[0].1.get("status").and_then(|v| v.as_str()),
            Some("completed")
        );
        assert_eq!(parsed[1].1.get("task_id").and_then(|v| v.as_i64()), Some(3));
        assert_eq!(
            parsed[1].1.get("status").and_then(|v| v.as_str()),
            Some("in_progress")
        );
    }

    #[test]
    fn applies_todo_status_update_from_task_id_args() {
        let current = vec![
            json!({"id":"todo-1","content":"a","status":"pending"}),
            json!({"id":"todo-2","content":"b","status":"pending"}),
            json!({"id":"todo-3","content":"c","status":"pending"}),
        ];
        let updated =
            apply_todo_updates_from_args(current, &json!({"task_id":2, "status":"completed"}))
                .expect("status update");
        assert_eq!(
            updated[1].get("status").and_then(|v| v.as_str()),
            Some("completed")
        );
    }
}
