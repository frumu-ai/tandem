use super::*;

impl EngineLoop {
    pub async fn run_prompt_async_with_context(
        &self,
        session_id: String,
        req: SendMessageRequest,
        correlation_id: Option<String>,
    ) -> anyhow::Result<()> {
        let session_model = self
            .storage
            .get_session(&session_id)
            .await
            .and_then(|s| s.model);
        let (provider_id, model_id_value) =
            resolve_model_route(req.model.as_ref(), session_model.as_ref()).ok_or_else(|| {
                anyhow::anyhow!(
                "MODEL_SELECTION_REQUIRED: explicit provider/model is required for this request."
            )
            })?;
        let correlation_ref = correlation_id.as_deref();
        let model_id = Some(model_id_value.as_str());
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
                provider_id: Some(provider_id.as_str()),
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
        let request_parts = req.parts.clone();
        let requested_tool_mode = req.tool_mode.clone().unwrap_or(ToolMode::Auto);
        let requested_context_mode = req.context_mode.clone().unwrap_or(ContextMode::Auto);
        let requested_write_required = req.write_required.unwrap_or(false);
        let requested_prewrite_requirements = req.prewrite_requirements.clone().unwrap_or_default();
        let request_tool_allowlist = req
            .tool_allowlist
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|tool| normalize_tool_name(&tool))
            .filter(|tool| !tool.trim().is_empty())
            .collect::<HashSet<_>>();
        // Propagate per-request tool allowlist to session-level enforcement so
        // that execution-time checks (and mcp_list scoping) also respect it.
        if !request_tool_allowlist.is_empty() {
            self.set_session_allowed_tools(
                &session_id,
                request_tool_allowlist.iter().cloned().collect(),
            )
            .await;
        }
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
        let runtime_attachments = build_runtime_attachments(&provider_id, &request_parts).await;
        self.auto_rename_session_from_user_text(&session_id, &text)
            .await;
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

        let mut question_tool_used = false;
        let completion = if let Some((tool, args)) = parse_tool_invocation(&text) {
            if normalize_tool_name(&tool) == "question" {
                question_tool_used = true;
            }
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
                    None,
                    active_agent.skills.as_deref(),
                    &text,
                    requested_write_required,
                    None,
                    cancel.clone(),
                )
                .await?
                .unwrap_or_default()
            }
        } else {
            let mut completion = String::new();
            let mut max_iterations = max_tool_iterations();
            let mut followup_context: Option<String> = None;
            let mut last_tool_outputs: Vec<String> = Vec::new();
            let mut tool_call_counts: HashMap<String, usize> = HashMap::new();
            let mut readonly_tool_cache: HashMap<String, String> = HashMap::new();
            let mut readonly_signature_counts: HashMap<String, usize> = HashMap::new();
            let mut mutable_signature_counts: HashMap<String, usize> = HashMap::new();
            let mut shell_mismatch_signatures: HashSet<String> = HashSet::new();
            let mut blocked_mcp_servers: HashSet<String> = HashSet::new();
            let mut websearch_query_blocked = false;
            let websearch_duplicate_signature_limit = websearch_duplicate_signature_limit();
            let mut pack_builder_executed = false;
            let mut auto_workspace_probe_attempted = false;
            let mut productive_tool_calls_total = 0usize;
            let mut productive_write_tool_calls_total = 0usize;
            let mut productive_workspace_inspection_total = 0usize;
            let mut productive_web_research_total = 0usize;
            let mut productive_concrete_read_total = 0usize;
            let mut successful_web_research_total = 0usize;
            let mut required_tool_retry_count = 0usize;
            let mut required_write_retry_count = 0usize;
            let mut unmet_prewrite_repair_retry_count = 0usize;
            let mut empty_completion_retry_count = 0usize;
            let mut prewrite_gate_waived = false;
            let mut invalid_tool_args_retry_count = 0usize;
            let strict_write_retry_max_attempts = strict_write_retry_max_attempts();
            let mut required_tool_unsatisfied_emitted = false;
            let mut latest_required_tool_failure_kind = RequiredToolFailureKind::NoToolCallEmitted;
            let email_delivery_requested = requires_email_delivery_prompt(&text);
            let web_research_requested = requires_web_research_prompt(&text);
            let code_workflow_requested = infer_code_workflow_from_text(&text);
            let mut email_action_executed = false;
            let mut latest_email_action_note: Option<String> = None;
            let mut email_tools_ever_offered = false;
            let intent = classify_intent(&text);
            let router_enabled = tool_router_enabled();
            let retrieval_enabled = semantic_tool_retrieval_enabled();
            let retrieval_k = semantic_tool_retrieval_k();
            let mcp_server_names = if mcp_catalog_in_system_prompt_enabled() {
                self.tools.mcp_server_names().await
            } else {
                Vec::new()
            };
            let mut auto_tools_escalated = matches!(requested_tool_mode, ToolMode::Required);
            let context_is_auto_compact = matches!(requested_context_mode, ContextMode::Auto)
                && runtime_attachments.is_empty()
                && is_short_simple_prompt(&text)
                && matches!(intent, ToolIntent::Chitchat | ToolIntent::Knowledge);

            while max_iterations > 0 && !cancel.is_cancelled() {
                let iteration = 26usize.saturating_sub(max_iterations);
                max_iterations -= 1;
                let context_profile = if matches!(requested_context_mode, ContextMode::Full) {
                    ChatHistoryProfile::Full
                } else if matches!(requested_context_mode, ContextMode::Compact)
                    || context_is_auto_compact
                {
                    ChatHistoryProfile::Compact
                } else {
                    ChatHistoryProfile::Standard
                };
                let mut messages =
                    load_chat_history(self.storage.clone(), &session_id, context_profile).await;
                if iteration == 1 && !runtime_attachments.is_empty() {
                    attach_to_last_user_message(&mut messages, &runtime_attachments);
                }
                let history_char_count = messages.iter().map(|m| m.content.len()).sum::<usize>();
                self.event_bus.publish(EngineEvent::new(
                    "context.profile.selected",
                    json!({
                        "sessionID": session_id,
                        "messageID": user_message_id,
                        "iteration": iteration,
                        "contextMode": format_context_mode(&requested_context_mode, context_is_auto_compact),
                        "historyMessageCount": messages.len(),
                        "historyCharCount": history_char_count,
                        "memoryInjected": false
                    }),
                ));
                let mut system_parts = vec![tandem_runtime_system_prompt(
                    &self.host_runtime_context,
                    &mcp_server_names,
                )];
                if let Some(system) = active_agent.system_prompt.as_ref() {
                    system_parts.push(system.clone());
                }
                messages.insert(
                    0,
                    ChatMessage {
                        role: "system".to_string(),
                        content: system_parts.join("\n\n"),
                        attachments: Vec::new(),
                    },
                );
                if let Some(extra) = followup_context.take() {
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: extra,
                        attachments: Vec::new(),
                    });
                }
                if let Some(hook) = self.prompt_context_hook.read().await.clone() {
                    let ctx = PromptContextHookContext {
                        session_id: session_id.clone(),
                        message_id: user_message_id.clone(),
                        provider_id: provider_id.clone(),
                        model_id: model_id_value.clone(),
                        iteration,
                    };
                    let hook_timeout =
                        Duration::from_millis(prompt_context_hook_timeout_ms() as u64);
                    match tokio::time::timeout(
                        hook_timeout,
                        hook.augment_provider_messages(ctx, messages.clone()),
                    )
                    .await
                    {
                        Ok(Ok(augmented)) => {
                            messages = augmented;
                        }
                        Ok(Err(err)) => {
                            self.event_bus.publish(EngineEvent::new(
                                "memory.context.error",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "error": truncate_text(&err.to_string(), 500),
                                }),
                            ));
                        }
                        Err(_) => {
                            self.event_bus.publish(EngineEvent::new(
                                "memory.context.error",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "error": format!(
                                        "prompt context hook timeout after {} ms",
                                        hook_timeout.as_millis()
                                    ),
                                }),
                            ));
                        }
                    }
                }
                let all_tools = self.tools.list().await;
                let mut retrieval_fallback_reason: Option<&'static str> = None;
                let mut candidate_tools = if retrieval_enabled {
                    self.tools.retrieve(&text, retrieval_k).await
                } else {
                    all_tools.clone()
                };
                if retrieval_enabled {
                    if candidate_tools.is_empty() && !all_tools.is_empty() {
                        candidate_tools = all_tools.clone();
                        retrieval_fallback_reason = Some("retrieval_empty_result");
                    } else if web_research_requested
                        && has_web_research_tools(&all_tools)
                        && !has_web_research_tools(&candidate_tools)
                        && required_write_retry_count == 0
                    {
                        candidate_tools = all_tools.clone();
                        retrieval_fallback_reason = Some("missing_web_tools_for_research_prompt");
                    } else if email_delivery_requested
                        && has_email_action_tools(&all_tools)
                        && !has_email_action_tools(&candidate_tools)
                    {
                        candidate_tools = all_tools.clone();
                        retrieval_fallback_reason = Some("missing_email_tools_for_delivery_prompt");
                    }
                }
                let mut tool_schemas = if !router_enabled {
                    candidate_tools
                } else {
                    match requested_tool_mode {
                        ToolMode::None => Vec::new(),
                        ToolMode::Required => select_tool_subset(
                            candidate_tools,
                            intent,
                            &request_tool_allowlist,
                            iteration > 1,
                        ),
                        ToolMode::Auto => {
                            if !auto_tools_escalated {
                                Vec::new()
                            } else {
                                select_tool_subset(
                                    candidate_tools,
                                    intent,
                                    &request_tool_allowlist,
                                    iteration > 1,
                                )
                            }
                        }
                    }
                };
                let mut policy_patterns =
                    request_tool_allowlist.iter().cloned().collect::<Vec<_>>();
                if let Some(agent_tools) = active_agent.tools.as_ref() {
                    policy_patterns
                        .extend(agent_tools.iter().map(|tool| normalize_tool_name(tool)));
                }
                let session_allowed_tools = self
                    .session_allowed_tools
                    .read()
                    .await
                    .get(&session_id)
                    .cloned()
                    .unwrap_or_default();
                policy_patterns.extend(session_allowed_tools.iter().cloned());
                if !policy_patterns.is_empty() {
                    let mut included = tool_schemas
                        .iter()
                        .map(|schema| normalize_tool_name(&schema.name))
                        .collect::<HashSet<_>>();
                    for schema in &all_tools {
                        let normalized = normalize_tool_name(&schema.name);
                        if policy_patterns
                            .iter()
                            .any(|pattern| tool_name_matches_policy(pattern, &normalized))
                            && included.insert(normalized)
                        {
                            tool_schemas.push(schema.clone());
                        }
                    }
                }
                if !request_tool_allowlist.is_empty() {
                    tool_schemas.retain(|schema| {
                        let tool = normalize_tool_name(&schema.name);
                        request_tool_allowlist
                            .iter()
                            .any(|pattern| tool_name_matches_policy(pattern, &tool))
                    });
                }
                let prewrite_gate = evaluate_prewrite_gate(
                    requested_write_required,
                    &requested_prewrite_requirements,
                    PrewriteProgress {
                        productive_write_tool_calls_total,
                        productive_workspace_inspection_total,
                        productive_concrete_read_total,
                        productive_web_research_total,
                        successful_web_research_total,
                        required_write_retry_count,
                        unmet_prewrite_repair_retry_count,
                        prewrite_gate_waived,
                    },
                );
                let _prewrite_satisfied = prewrite_gate.prewrite_satisfied;
                let prewrite_gate_write = prewrite_gate.gate_write;
                let force_write_only_retry = prewrite_gate.force_write_only_retry;
                let allow_repair_tools = prewrite_gate.allow_repair_tools;
                if prewrite_gate_write {
                    tool_schemas.retain(|schema| !is_workspace_write_tool(&schema.name));
                }
                if requested_prewrite_requirements.repair_on_unmet_requirements
                    && productive_write_tool_calls_total >= 3
                {
                    tool_schemas.retain(|schema| !is_workspace_write_tool(&schema.name));
                }
                if allow_repair_tools {
                    let unmet_prewrite_codes = prewrite_gate.unmet_codes.clone();
                    let repair_tools = tool_schemas
                        .iter()
                        .filter(|schema| {
                            tool_matches_unmet_prewrite_repair_requirement(
                                &schema.name,
                                &unmet_prewrite_codes,
                            )
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !repair_tools.is_empty() {
                        tool_schemas = repair_tools;
                    }
                }
                if force_write_only_retry && !allow_repair_tools {
                    tool_schemas.retain(|schema| is_workspace_write_tool(&schema.name));
                }
                if active_agent.tools.is_some() {
                    tool_schemas.retain(|schema| agent_can_use_tool(&active_agent, &schema.name));
                }
                tool_schemas.retain(|schema| {
                    let normalized = normalize_tool_name(&schema.name);
                    if let Some(server) = mcp_server_from_tool_name(&normalized) {
                        !blocked_mcp_servers.contains(server)
                    } else {
                        true
                    }
                });
                if let Some(allowed_tools) = self
                    .session_allowed_tools
                    .read()
                    .await
                    .get(&session_id)
                    .cloned()
                {
                    if !allowed_tools.is_empty() {
                        tool_schemas.retain(|schema| {
                            let normalized = normalize_tool_name(&schema.name);
                            any_policy_matches(&allowed_tools, &normalized)
                        });
                    }
                }
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
                            provider_id: Some(provider_id.as_str()),
                            model_id,
                            status: Some("failed"),
                            error_code: Some("TOOL_SCHEMA_INVALID"),
                            detail: Some(&detail),
                        },
                    );
                    anyhow::bail!("{detail}");
                }
                let routing_decision = ToolRoutingDecision {
                    pass: if auto_tools_escalated { 2 } else { 1 },
                    mode: match requested_tool_mode {
                        ToolMode::Auto => default_mode_name(),
                        ToolMode::None => "none",
                        ToolMode::Required => "required",
                    },
                    intent,
                    selected_count: tool_schemas.len(),
                    total_available_count: all_tools.len(),
                    mcp_included: tool_schemas
                        .iter()
                        .any(|schema| normalize_tool_name(&schema.name).starts_with("mcp.")),
                };
                self.event_bus.publish(EngineEvent::new(
                    "tool.routing.decision",
                    json!({
                        "sessionID": session_id,
                        "messageID": user_message_id,
                        "iteration": iteration,
                        "pass": routing_decision.pass,
                        "mode": routing_decision.mode,
                        "intent": format!("{:?}", routing_decision.intent).to_ascii_lowercase(),
                        "selectedToolCount": routing_decision.selected_count,
                        "totalAvailableTools": routing_decision.total_available_count,
                        "mcpIncluded": routing_decision.mcp_included,
                        "retrievalEnabled": retrieval_enabled,
                        "retrievalK": retrieval_k,
                        "fallbackToFullTools": retrieval_fallback_reason.is_some(),
                        "fallbackReason": retrieval_fallback_reason
                    }),
                ));
                let allowed_tool_names = tool_schemas
                    .iter()
                    .map(|schema| normalize_tool_name(&schema.name))
                    .collect::<HashSet<_>>();
                if !email_tools_ever_offered && has_email_action_tools(&tool_schemas) {
                    email_tools_ever_offered = true;
                }
                let offered_tool_preview = tool_schemas
                    .iter()
                    .take(8)
                    .map(|schema| normalize_tool_name(&schema.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.event_bus.publish(EngineEvent::new(
                    "provider.call.iteration.start",
                    json!({
                        "sessionID": session_id,
                        "messageID": user_message_id,
                        "iteration": iteration,
                        "selectedToolCount": allowed_tool_names.len(),
                    }),
                ));
                let estimated_prompt_chars: usize = messages.iter().map(|m| m.content.len()).sum();
                let provider_connect_timeout =
                    Duration::from_millis(provider_stream_connect_timeout_ms() as u64);
                let stream_result = tokio::time::timeout(
                    provider_connect_timeout,
                    self.providers.stream_for_provider(
                        Some(provider_id.as_str()),
                        Some(model_id_value.as_str()),
                        messages,
                        requested_tool_mode.clone(),
                        Some(tool_schemas),
                        cancel.clone(),
                    ),
                )
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "provider stream connect timeout after {} ms",
                        provider_connect_timeout.as_millis()
                    )
                })
                .and_then(|result| result);
                let stream = match stream_result {
                    Ok(stream) => stream,
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
                                provider_id: Some(provider_id.as_str()),
                                model_id,
                                status: Some("failed"),
                                error_code: Some(error_code),
                                detail: Some(&detail),
                            },
                        );
                        self.event_bus.publish(EngineEvent::new(
                            "provider.call.iteration.error",
                            json!({
                                "sessionID": session_id,
                                "messageID": user_message_id,
                                "iteration": iteration,
                                "error": detail,
                            }),
                        ));
                        return Err(err);
                    }
                };
                tokio::pin!(stream);
                completion.clear();
                let mut streamed_tool_calls: HashMap<String, StreamedToolCall> = HashMap::new();
                let mut provider_usage: Option<TokenUsage> = None;
                let mut accepted_tool_calls_in_cycle = 0usize;
                let provider_idle_timeout =
                    Duration::from_millis(provider_stream_idle_timeout_ms() as u64);
                loop {
                    let next_chunk_result =
                        tokio::time::timeout(provider_idle_timeout, stream.next())
                            .await
                            .map_err(|_| {
                                anyhow::anyhow!(
                                    "provider stream idle timeout after {} ms",
                                    provider_idle_timeout.as_millis()
                                )
                            });
                    let next_chunk = match next_chunk_result {
                        Ok(next_chunk) => next_chunk,
                        Err(err) => {
                            self.event_bus.publish(EngineEvent::new(
                                "provider.call.iteration.error",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "error": truncate_text(&err.to_string(), 500),
                                }),
                            ));
                            return Err(err);
                        }
                    };
                    let Some(chunk) = next_chunk else {
                        break;
                    };
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
                                    provider_id: Some(provider_id.as_str()),
                                    model_id,
                                    status: Some("failed"),
                                    error_code: Some(error_code),
                                    detail: Some(&detail),
                                },
                            );
                            self.event_bus.publish(EngineEvent::new(
                                "provider.call.iteration.error",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "error": detail,
                                }),
                            ));
                            return Err(anyhow::anyhow!(
                                "provider stream chunk error: {error_text}"
                            ));
                        }
                    };
                    match chunk {
                        StreamChunk::TextDelta(delta) => {
                            let delta = strip_model_control_markers(&delta);
                            if delta.trim().is_empty() {
                                continue;
                            }
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
                                        provider_id: Some(provider_id.as_str()),
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
                        StreamChunk::Done {
                            finish_reason: _,
                            usage,
                        } => {
                            if usage.is_some() {
                                provider_usage = usage;
                            }
                            break;
                        }
                        StreamChunk::ToolCallStart { id, name } => {
                            let entry = streamed_tool_calls.entry(id).or_default();
                            if entry.name.is_empty() {
                                entry.name = name;
                            }
                        }
                        StreamChunk::ToolCallDelta { id, args_delta } => {
                            let entry = streamed_tool_calls.entry(id.clone()).or_default();
                            entry.args.push_str(&args_delta);
                            let tool_name = if entry.name.trim().is_empty() {
                                "tool".to_string()
                            } else {
                                normalize_tool_name(&entry.name)
                            };
                            let parsed_preview = if entry.name.trim().is_empty() {
                                Value::String(truncate_text(&entry.args, 1_000))
                            } else {
                                parse_streamed_tool_args(&tool_name, &entry.args)
                            };
                            let mut tool_part = WireMessagePart::tool_invocation(
                                &session_id,
                                &user_message_id,
                                tool_name.clone(),
                                parsed_preview.clone(),
                            );
                            tool_part.id = Some(id.clone());
                            if tool_name == "write" {
                                tracing::info!(
                                    session_id = %session_id,
                                    message_id = %user_message_id,
                                    tool_call_id = %id,
                                    args_delta_len = args_delta.len(),
                                    accumulated_args_len = entry.args.len(),
                                    parsed_preview_empty = parsed_preview.is_null()
                                        || parsed_preview.as_object().is_some_and(|value| value.is_empty())
                                        || parsed_preview
                                            .as_str()
                                            .map(|value| value.trim().is_empty())
                                            .unwrap_or(false),
                                    "streamed write tool args delta received"
                                );
                            }
                            self.event_bus.publish(EngineEvent::new(
                                "message.part.updated",
                                json!({
                                    "part": tool_part,
                                    "toolCallDelta": {
                                        "id": id,
                                        "tool": tool_name,
                                        "argsDelta": truncate_text(&args_delta, 1_000),
                                        "rawArgsPreview": truncate_text(&entry.args, 2_000),
                                        "parsedArgsPreview": parsed_preview
                                    }
                                }),
                            ));
                        }
                        StreamChunk::ToolCallEnd { id: _ } => {}
                    }
                    if cancel.is_cancelled() {
                        break;
                    }
                }

                let streamed_tool_call_count = streamed_tool_calls.len();
                let streamed_tool_call_parse_failed = streamed_tool_calls
                    .values()
                    .any(|call| !call.args.trim().is_empty() && call.name.trim().is_empty());
                let mut tool_calls = streamed_tool_calls
                    .into_iter()
                    .filter_map(|(call_id, call)| {
                        if call.name.trim().is_empty() {
                            return None;
                        }
                        let tool_name = normalize_tool_name(&call.name);
                        let parsed_args = parse_streamed_tool_args(&tool_name, &call.args);
                        Some(ParsedToolCall {
                            tool: tool_name,
                            args: parsed_args,
                            call_id: Some(call_id),
                        })
                    })
                    .collect::<Vec<_>>();
                if tool_calls.is_empty() {
                    tool_calls = parse_tool_invocations_from_response(&completion)
                        .into_iter()
                        .map(|(tool, args)| ParsedToolCall {
                            tool,
                            args,
                            call_id: None,
                        })
                        .collect::<Vec<_>>();
                }
                let provider_tool_parse_failed = tool_calls.is_empty()
                    && (streamed_tool_call_parse_failed
                        || (streamed_tool_call_count > 0
                            && looks_like_unparsed_tool_payload(&completion))
                        || looks_like_unparsed_tool_payload(&completion));
                if provider_tool_parse_failed {
                    latest_required_tool_failure_kind =
                        RequiredToolFailureKind::ToolCallParseFailed;
                } else if tool_calls.is_empty() {
                    latest_required_tool_failure_kind = RequiredToolFailureKind::NoToolCallEmitted;
                }
                if router_enabled
                    && matches!(requested_tool_mode, ToolMode::Auto)
                    && !auto_tools_escalated
                    && iteration == 1
                    && should_escalate_auto_tools(intent, &text, &completion)
                {
                    auto_tools_escalated = true;
                    followup_context = Some(
                        "Tool access is now enabled for this request. Use only necessary tools and then answer concisely."
                            .to_string(),
                    );
                    self.event_bus.publish(EngineEvent::new(
                        "provider.call.iteration.finish",
                        json!({
                            "sessionID": session_id,
                            "messageID": user_message_id,
                            "iteration": iteration,
                            "finishReason": "auto_escalate",
                            "acceptedToolCalls": accepted_tool_calls_in_cycle,
                            "rejectedToolCalls": 0,
                        }),
                    ));
                    continue;
                }
                if tool_calls.is_empty()
                    && !auto_workspace_probe_attempted
                    && should_force_workspace_probe(&text, &completion)
                    && allowed_tool_names.contains("glob")
                {
                    auto_workspace_probe_attempted = true;
                    tool_calls = vec![ParsedToolCall {
                        tool: "glob".to_string(),
                        args: json!({ "pattern": "*" }),
                        call_id: None,
                    }];
                }
                if !tool_calls.is_empty() {
                    let saw_tool_call_candidate = true;
                    let mut outputs = Vec::new();
                    let mut executed_productive_tool = false;
                    let mut write_tool_attempted_in_cycle = false;
                    let mut auth_required_hit_in_cycle = false;
                    let mut guard_budget_hit_in_cycle = false;
                    let mut duplicate_signature_hit_in_cycle = false;
                    let mut rejected_tool_call_in_cycle = false;
                    for ParsedToolCall {
                        tool,
                        args,
                        call_id,
                    } in tool_calls
                    {
                        if !agent_can_use_tool(&active_agent, &tool) {
                            rejected_tool_call_in_cycle = true;
                            continue;
                        }
                        let tool_key = normalize_tool_name(&tool);
                        if is_workspace_write_tool(&tool_key) {
                            write_tool_attempted_in_cycle = true;
                        }
                        if !allowed_tool_names.contains(&tool_key) {
                            rejected_tool_call_in_cycle = true;
                            let note = if offered_tool_preview.is_empty() {
                                format!(
                                    "Tool `{}` call skipped: it is not available in this turn.",
                                    tool_key
                                )
                            } else {
                                format!(
                                    "Tool `{}` call skipped: it is not available in this turn. Available tools: {}.",
                                    tool_key, offered_tool_preview
                                )
                            };
                            self.event_bus.publish(EngineEvent::new(
                                "tool.call.rejected_unoffered",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "tool": tool_key,
                                    "offeredToolCount": allowed_tool_names.len()
                                }),
                            ));
                            if tool_name_looks_like_email_action(&tool_key) {
                                latest_email_action_note = Some(note.clone());
                            }
                            outputs.push(note);
                            continue;
                        }
                        if let Some(server) = mcp_server_from_tool_name(&tool_key) {
                            if blocked_mcp_servers.contains(server) {
                                rejected_tool_call_in_cycle = true;
                                outputs.push(format!(
                                    "Tool `{}` call skipped: authorization is still pending for MCP server `{}`.",
                                    tool_key, server
                                ));
                                continue;
                            }
                        }
                        if tool_key == "question" {
                            question_tool_used = true;
                        }
                        if tool_key == "pack_builder" && pack_builder_executed {
                            rejected_tool_call_in_cycle = true;
                            outputs.push(
                                "Tool `pack_builder` call skipped: already executed in this run. Provide a final response or ask any required follow-up question."
                                    .to_string(),
                            );
                            continue;
                        }
                        if websearch_query_blocked && tool_key == "websearch" {
                            rejected_tool_call_in_cycle = true;
                            outputs.push(
                                "Tool `websearch` call skipped: WEBSEARCH_QUERY_MISSING"
                                    .to_string(),
                            );
                            continue;
                        }
                        let mut effective_args = args.clone();
                        if tool_key == "todo_write" {
                            effective_args = normalize_todo_write_args(effective_args, &completion);
                            if is_empty_todo_write_args(&effective_args) {
                                rejected_tool_call_in_cycle = true;
                                outputs.push(
                                    "Tool `todo_write` call skipped: empty todo payload."
                                        .to_string(),
                                );
                                continue;
                            }
                        }
                        let signature = if tool_key == "batch" {
                            batch_tool_signature(&args)
                                .unwrap_or_else(|| tool_signature(&tool_key, &args))
                        } else {
                            tool_signature(&tool_key, &args)
                        };
                        if is_shell_tool_name(&tool_key)
                            && shell_mismatch_signatures.contains(&signature)
                        {
                            rejected_tool_call_in_cycle = true;
                            outputs.push(
                                "Tool `bash` call skipped: previous invocation hit an OS/path mismatch. Use `read`, `glob`, or `grep`."
                                    .to_string(),
                            );
                            continue;
                        }
                        let mut signature_count = 1usize;
                        if is_read_only_tool(&tool_key)
                            || (tool_key == "batch" && is_read_only_batch_call(&args))
                        {
                            let count = readonly_signature_counts
                                .entry(signature.clone())
                                .and_modify(|v| *v = v.saturating_add(1))
                                .or_insert(1);
                            signature_count = *count;
                            if tool_key == "websearch" {
                                if let Some(limit) = websearch_duplicate_signature_limit {
                                    if *count > limit {
                                        rejected_tool_call_in_cycle = true;
                                        self.event_bus.publish(EngineEvent::new(
                                            "tool.loop_guard.triggered",
                                            json!({
                                                "sessionID": session_id,
                                                "messageID": user_message_id,
                                                "tool": tool_key,
                                                "reason": "duplicate_signature_retry_exhausted",
                                                "duplicateLimit": limit,
                                                "queryHash": extract_websearch_query(&args).map(|q| stable_hash(&q)),
                                                "loop_guard_triggered": true
                                            }),
                                        ));
                                        outputs.push(
                                            "Tool `websearch` call skipped: WEBSEARCH_LOOP_GUARD"
                                                .to_string(),
                                        );
                                        continue;
                                    }
                                }
                            }
                            if tool_key != "websearch" && *count > 1 {
                                rejected_tool_call_in_cycle = true;
                                if let Some(cached) = readonly_tool_cache.get(&signature) {
                                    outputs.push(cached.clone());
                                } else {
                                    outputs.push(format!(
                                        "Tool `{}` call skipped: duplicate call signature detected.",
                                        tool_key
                                    ));
                                }
                                continue;
                            }
                        }
                        let is_read_only_signature = is_read_only_tool(&tool_key)
                            || (tool_key == "batch" && is_read_only_batch_call(&args));
                        if !is_read_only_signature {
                            let duplicate_limit = duplicate_signature_limit_for(&tool_key);
                            let seen = mutable_signature_counts
                                .entry(signature.clone())
                                .and_modify(|v| *v = v.saturating_add(1))
                                .or_insert(1);
                            if *seen > duplicate_limit {
                                rejected_tool_call_in_cycle = true;
                                self.event_bus.publish(EngineEvent::new(
                                    "tool.loop_guard.triggered",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "tool": tool_key,
                                        "reason": "duplicate_signature_retry_exhausted",
                                        "signatureHash": stable_hash(&signature),
                                        "duplicateLimit": duplicate_limit,
                                        "loop_guard_triggered": true
                                    }),
                                ));
                                outputs.push(format!(
                                    "Tool `{}` call skipped: duplicate call signature retry limit reached ({}).",
                                    tool_key, duplicate_limit
                                ));
                                duplicate_signature_hit_in_cycle = true;
                                continue;
                            }
                        }
                        let budget = tool_budget_for(&tool_key);
                        let entry = tool_call_counts.entry(tool_key.clone()).or_insert(0);
                        if *entry >= budget {
                            rejected_tool_call_in_cycle = true;
                            outputs.push(format!(
                                "Tool `{}` call skipped: per-run guard budget exceeded ({}).",
                                tool_key, budget
                            ));
                            guard_budget_hit_in_cycle = true;
                            continue;
                        }
                        let mut finalized_part = WireMessagePart::tool_invocation(
                            &session_id,
                            &user_message_id,
                            tool.clone(),
                            effective_args.clone(),
                        );
                        if let Some(call_id) = call_id.clone() {
                            finalized_part.id = Some(call_id);
                        }
                        finalized_part.state = Some("pending".to_string());
                        self.event_bus.publish(EngineEvent::new(
                            "message.part.updated",
                            json!({"part": finalized_part}),
                        ));
                        *entry += 1;
                        accepted_tool_calls_in_cycle =
                            accepted_tool_calls_in_cycle.saturating_add(1);
                        if let Some(output) = self
                            .execute_tool_with_permission(
                                &session_id,
                                &user_message_id,
                                tool,
                                effective_args,
                                call_id,
                                active_agent.skills.as_deref(),
                                &text,
                                requested_write_required,
                                Some(&completion),
                                cancel.clone(),
                            )
                            .await?
                        {
                            let productive = is_productive_tool_output(&tool_key, &output);
                            if output.contains("WEBSEARCH_QUERY_MISSING") {
                                websearch_query_blocked = true;
                            }
                            if is_shell_tool_name(&tool_key) && is_os_mismatch_tool_output(&output)
                            {
                                shell_mismatch_signatures.insert(signature.clone());
                            }
                            if is_read_only_tool(&tool_key)
                                && tool_key != "websearch"
                                && signature_count == 1
                            {
                                readonly_tool_cache.insert(signature, output.clone());
                            }
                            if productive {
                                productive_tool_calls_total =
                                    productive_tool_calls_total.saturating_add(1);
                                if is_workspace_write_tool(&tool_key) {
                                    productive_write_tool_calls_total =
                                        productive_write_tool_calls_total.saturating_add(1);
                                }
                                if is_workspace_inspection_tool(&tool_key) {
                                    productive_workspace_inspection_total =
                                        productive_workspace_inspection_total.saturating_add(1);
                                }
                                if tool_key == "read" {
                                    productive_concrete_read_total =
                                        productive_concrete_read_total.saturating_add(1);
                                }
                                if is_web_research_tool(&tool_key) {
                                    productive_web_research_total =
                                        productive_web_research_total.saturating_add(1);
                                    if is_successful_web_research_output(&tool_key, &output) {
                                        successful_web_research_total =
                                            successful_web_research_total.saturating_add(1);
                                    }
                                }
                                executed_productive_tool = true;
                                if tool_key == "pack_builder" {
                                    pack_builder_executed = true;
                                }
                            }
                            if tool_name_looks_like_email_action(&tool_key) {
                                if productive {
                                    email_action_executed = true;
                                } else {
                                    latest_email_action_note =
                                        Some(truncate_text(&output, 280).replace('\n', " "));
                                }
                            }
                            if is_auth_required_tool_output(&output) {
                                if let Some(server) = mcp_server_from_tool_name(&tool_key) {
                                    blocked_mcp_servers.insert(server.to_string());
                                }
                                auth_required_hit_in_cycle = true;
                            }
                            outputs.push(output);
                            if auth_required_hit_in_cycle {
                                break;
                            }
                            if guard_budget_hit_in_cycle {
                                break;
                            }
                        }
                    }
                    if !outputs.is_empty() {
                        last_tool_outputs = outputs.clone();
                        if matches!(requested_tool_mode, ToolMode::Required)
                            && productive_tool_calls_total == 0
                        {
                            latest_required_tool_failure_kind = classify_required_tool_failure(
                                &outputs,
                                saw_tool_call_candidate,
                                accepted_tool_calls_in_cycle,
                                provider_tool_parse_failed,
                                rejected_tool_call_in_cycle,
                            );
                            if requested_write_required
                                && write_tool_attempted_in_cycle
                                && productive_write_tool_calls_total == 0
                                && is_write_invalid_args_failure_kind(
                                    latest_required_tool_failure_kind,
                                )
                            {
                                if required_write_retry_count + 1 < strict_write_retry_max_attempts
                                {
                                    required_write_retry_count += 1;
                                    required_tool_retry_count += 1;
                                    followup_context = Some(build_write_required_retry_context(
                                        &offered_tool_preview,
                                        latest_required_tool_failure_kind,
                                        &text,
                                        &requested_prewrite_requirements,
                                        productive_workspace_inspection_total > 0,
                                        productive_concrete_read_total > 0,
                                        productive_web_research_total > 0,
                                        successful_web_research_total > 0,
                                    ));
                                    self.event_bus.publish(EngineEvent::new(
                                        "provider.call.iteration.finish",
                                        json!({
                                            "sessionID": session_id,
                                            "messageID": user_message_id,
                                            "iteration": iteration,
                                            "finishReason": "required_write_invalid_retry",
                                            "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                            "rejectedToolCalls": 0,
                                            "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                                        }),
                                    ));
                                    continue;
                                }
                            }
                            let progress_made_in_cycle = productive_workspace_inspection_total > 0
                                || productive_concrete_read_total > 0
                                || productive_web_research_total > 0
                                || successful_web_research_total > 0;
                            if should_retry_nonproductive_required_tool_cycle(
                                requested_write_required,
                                write_tool_attempted_in_cycle,
                                progress_made_in_cycle,
                                required_tool_retry_count,
                            ) {
                                required_tool_retry_count += 1;
                                followup_context =
                                    Some(build_required_tool_retry_context_for_task(
                                        &offered_tool_preview,
                                        latest_required_tool_failure_kind,
                                        &text,
                                    ));
                                self.event_bus.publish(EngineEvent::new(
                                    "provider.call.iteration.finish",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "finishReason": "required_tool_retry",
                                        "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                        "rejectedToolCalls": 0,
                                        "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                                    }),
                                ));
                                continue;
                            }
                            completion = required_tool_mode_unsatisfied_completion(
                                latest_required_tool_failure_kind,
                            );
                            if !required_tool_unsatisfied_emitted {
                                required_tool_unsatisfied_emitted = true;
                                self.event_bus.publish(EngineEvent::new(
                                    "tool.mode.required.unsatisfied",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "selectedToolCount": allowed_tool_names.len(),
                                        "offeredToolsPreview": offered_tool_preview,
                                        "reason": latest_required_tool_failure_kind.code(),
                                    }),
                                ));
                            }
                            self.event_bus.publish(EngineEvent::new(
                                "provider.call.iteration.finish",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "finishReason": "required_tool_unsatisfied",
                                    "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                    "rejectedToolCalls": 0,
                                    "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                                }),
                            ));
                            break;
                        }
                        let prewrite_gate = evaluate_prewrite_gate(
                            requested_write_required,
                            &requested_prewrite_requirements,
                            PrewriteProgress {
                                productive_write_tool_calls_total,
                                productive_workspace_inspection_total,
                                productive_concrete_read_total,
                                productive_web_research_total,
                                successful_web_research_total,
                                required_write_retry_count,
                                unmet_prewrite_repair_retry_count,
                                prewrite_gate_waived,
                            },
                        );
                        let prewrite_satisfied = prewrite_gate.prewrite_satisfied;
                        let unmet_prewrite_codes = prewrite_gate.unmet_codes.clone();
                        if requested_write_required
                            && productive_tool_calls_total > 0
                            && productive_write_tool_calls_total == 0
                        {
                            if should_start_prewrite_repair_before_first_write(
                                requested_prewrite_requirements.repair_on_unmet_requirements,
                                productive_write_tool_calls_total,
                                prewrite_satisfied,
                                code_workflow_requested,
                            ) {
                                if unmet_prewrite_repair_retry_count
                                    < prewrite_repair_retry_max_attempts()
                                {
                                    unmet_prewrite_repair_retry_count += 1;
                                    let repair_attempt = unmet_prewrite_repair_retry_count;
                                    let repair_attempts_remaining =
                                        prewrite_repair_retry_max_attempts()
                                            .saturating_sub(repair_attempt);
                                    followup_context = Some(build_prewrite_repair_retry_context(
                                        &offered_tool_preview,
                                        latest_required_tool_failure_kind,
                                        &text,
                                        &requested_prewrite_requirements,
                                        productive_workspace_inspection_total > 0,
                                        productive_concrete_read_total > 0,
                                        productive_web_research_total > 0,
                                        successful_web_research_total > 0,
                                    ));
                                    self.event_bus.publish(EngineEvent::new(
                                        "provider.call.iteration.finish",
                                        json!({
                                            "sessionID": session_id,
                                            "messageID": user_message_id,
                                            "iteration": iteration,
                                            "finishReason": "prewrite_repair_retry",
                                            "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                            "rejectedToolCalls": 0,
                                            "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                                            "repair": prewrite_repair_event_payload(
                                                repair_attempt,
                                                repair_attempts_remaining,
                                                &unmet_prewrite_codes,
                                                false,
                                            ),
                                        }),
                                    ));
                                    continue;
                                }
                                if !prewrite_gate_waived {
                                    if prewrite_gate_strict_mode() {
                                        // Strict mode: refuse to waive; emit blocked event and
                                        // continue so the gate retains control.
                                        self.event_bus.publish(EngineEvent::new(
                                            "prewrite.gate.strict_mode.blocked",
                                            json!({
                                                "sessionID": session_id,
                                                "messageID": user_message_id,
                                                "iteration": iteration,
                                                "unmetCodes": unmet_prewrite_codes,
                                            }),
                                        ));
                                        continue;
                                    }
                                    prewrite_gate_waived = true;
                                    let repair_attempt = unmet_prewrite_repair_retry_count;
                                    let repair_attempts_remaining =
                                        prewrite_repair_retry_max_attempts()
                                            .saturating_sub(repair_attempt);
                                    followup_context = Some(build_prewrite_waived_write_context(
                                        &text,
                                        &unmet_prewrite_codes,
                                    ));
                                    self.event_bus.publish(EngineEvent::new(
                                        "prewrite.gate.waived.write_executed",
                                        json!({
                                            "sessionID": session_id,
                                            "messageID": user_message_id,
                                            "unmetCodes": unmet_prewrite_codes,
                                        }),
                                    ));
                                    self.event_bus.publish(EngineEvent::new(
                                        "provider.call.iteration.finish",
                                        json!({
                                            "sessionID": session_id,
                                            "messageID": user_message_id,
                                            "iteration": iteration,
                                            "finishReason": "prewrite_gate_waived",
                                            "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                            "rejectedToolCalls": 0,
                                            "prewriteGateWaived": true,
                                            "repair": prewrite_repair_event_payload(
                                                repair_attempt,
                                                repair_attempts_remaining,
                                                &unmet_prewrite_codes,
                                                true,
                                            ),
                                        }),
                                    ));
                                    continue;
                                }
                            }
                            latest_required_tool_failure_kind =
                                RequiredToolFailureKind::WriteRequiredNotSatisfied;
                            if required_write_retry_count + 1 < strict_write_retry_max_attempts {
                                required_write_retry_count += 1;
                                followup_context = Some(build_write_required_retry_context(
                                    &offered_tool_preview,
                                    latest_required_tool_failure_kind,
                                    &text,
                                    &requested_prewrite_requirements,
                                    productive_workspace_inspection_total > 0,
                                    productive_concrete_read_total > 0,
                                    productive_web_research_total > 0,
                                    successful_web_research_total > 0,
                                ));
                                self.event_bus.publish(EngineEvent::new(
                                    "provider.call.iteration.finish",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "finishReason": "required_write_retry",
                                        "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                        "rejectedToolCalls": 0,
                                        "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                                    }),
                                ));
                                continue;
                            }
                            completion = required_tool_mode_unsatisfied_completion(
                                latest_required_tool_failure_kind,
                            );
                            if !required_tool_unsatisfied_emitted {
                                required_tool_unsatisfied_emitted = true;
                                self.event_bus.publish(EngineEvent::new(
                                    "tool.mode.required.unsatisfied",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "selectedToolCount": allowed_tool_names.len(),
                                        "offeredToolsPreview": offered_tool_preview,
                                        "reason": latest_required_tool_failure_kind.code(),
                                    }),
                                ));
                            }
                            self.event_bus.publish(EngineEvent::new(
                                "provider.call.iteration.finish",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "finishReason": "required_write_unsatisfied",
                                    "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                    "rejectedToolCalls": 0,
                                    "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                                }),
                            ));
                            break;
                        }
                        if invalid_tool_args_retry_count < invalid_tool_args_retry_max_attempts() {
                            if let Some(retry_context) =
                                build_invalid_tool_args_retry_context_from_outputs(
                                    &outputs,
                                    invalid_tool_args_retry_count,
                                )
                            {
                                invalid_tool_args_retry_count += 1;
                                followup_context = Some(format!(
                                    "Previous tool call arguments were invalid. {}",
                                    retry_context
                                ));
                                self.event_bus.publish(EngineEvent::new(
                                    "provider.call.iteration.finish",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "finishReason": "invalid_tool_args_retry",
                                        "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                        "rejectedToolCalls": 0,
                                    }),
                                ));
                                continue;
                            }
                        }
                        let guard_budget_hit =
                            outputs.iter().any(|o| is_guard_budget_tool_output(o));
                        if executed_productive_tool {
                            let prewrite_gate = evaluate_prewrite_gate(
                                requested_write_required,
                                &requested_prewrite_requirements,
                                PrewriteProgress {
                                    productive_write_tool_calls_total,
                                    productive_workspace_inspection_total,
                                    productive_concrete_read_total,
                                    productive_web_research_total,
                                    successful_web_research_total,
                                    required_write_retry_count,
                                    unmet_prewrite_repair_retry_count,
                                    prewrite_gate_waived,
                                },
                            );
                            let prewrite_satisfied = prewrite_gate.prewrite_satisfied;
                            let unmet_prewrite_codes = prewrite_gate.unmet_codes.clone();
                            if requested_write_required
                                && productive_write_tool_calls_total > 0
                                && requested_prewrite_requirements.repair_on_unmet_requirements
                                && unmet_prewrite_repair_retry_count
                                    < prewrite_repair_retry_max_attempts()
                                && !prewrite_satisfied
                            {
                                unmet_prewrite_repair_retry_count += 1;
                                let repair_attempt = unmet_prewrite_repair_retry_count;
                                let repair_attempts_remaining =
                                    prewrite_repair_retry_max_attempts()
                                        .saturating_sub(repair_attempt);
                                followup_context = Some(build_prewrite_repair_retry_context(
                                    &offered_tool_preview,
                                    latest_required_tool_failure_kind,
                                    &text,
                                    &requested_prewrite_requirements,
                                    productive_workspace_inspection_total > 0,
                                    productive_concrete_read_total > 0,
                                    productive_web_research_total > 0,
                                    successful_web_research_total > 0,
                                ));
                                self.event_bus.publish(EngineEvent::new(
                                    "provider.call.iteration.finish",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "finishReason": "prewrite_repair_retry",
                                        "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                        "rejectedToolCalls": 0,
                                        "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                                        "repair": prewrite_repair_event_payload(
                                            repair_attempt,
                                            repair_attempts_remaining,
                                            &unmet_prewrite_codes,
                                            false,
                                        ),
                                    }),
                                ));
                                continue;
                            }
                            followup_context = Some(format!(
                                "{}\nContinue with a concise final response and avoid repeating identical tool calls.",
                                summarize_tool_outputs(&outputs)
                            ));
                            self.event_bus.publish(EngineEvent::new(
                                "provider.call.iteration.finish",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "finishReason": "tool_followup",
                                    "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                    "rejectedToolCalls": 0,
                                }),
                            ));
                            continue;
                        }
                        if guard_budget_hit {
                            completion = summarize_guard_budget_outputs(&outputs)
                                .unwrap_or_else(|| {
                                    "This run hit the per-run tool guard budget, so tool execution was paused to avoid retries. Send a new message to start a fresh run.".to_string()
                                });
                        } else if duplicate_signature_hit_in_cycle {
                            completion = summarize_duplicate_signature_outputs(&outputs)
                                .unwrap_or_else(|| {
                                    "This run paused because the same tool call kept repeating. Rephrase the request or provide a different command target and retry.".to_string()
                                });
                        } else if let Some(summary) = summarize_auth_pending_outputs(&outputs) {
                            completion = summary;
                        } else {
                            completion.clear();
                        }
                        self.event_bus.publish(EngineEvent::new(
                            "provider.call.iteration.finish",
                            json!({
                                "sessionID": session_id,
                                "messageID": user_message_id,
                                "iteration": iteration,
                                "finishReason": "tool_summary",
                                "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                "rejectedToolCalls": 0,
                            }),
                        ));
                        break;
                    } else if matches!(requested_tool_mode, ToolMode::Required) {
                        latest_required_tool_failure_kind = classify_required_tool_failure(
                            &outputs,
                            saw_tool_call_candidate,
                            accepted_tool_calls_in_cycle,
                            provider_tool_parse_failed,
                            rejected_tool_call_in_cycle,
                        );
                    }
                }

                {
                    let (prompt_tokens, completion_tokens, total_tokens, usage_source) =
                        if let Some(usage) = provider_usage {
                            (
                                usage.prompt_tokens,
                                usage.completion_tokens,
                                usage.total_tokens,
                                "provider",
                            )
                        } else {
                            // Provider did not return usage (e.g. streaming without
                            // include_usage, or a backend that omits it). Estimate from
                            // accumulated char counts using the ~4 chars-per-token heuristic.
                            let est_prompt = (estimated_prompt_chars / 4) as u64;
                            let est_completion = (completion.len() / 4) as u64;
                            tracing::debug!(
                                session_id = %session_id,
                                provider_id = %provider_id,
                                "provider.usage missing from stream; using char-count estimate \
                                 (prompt_chars={estimated_prompt_chars} completion_chars={})",
                                completion.len()
                            );
                            (
                                est_prompt,
                                est_completion,
                                est_prompt.saturating_add(est_completion),
                                "estimated",
                            )
                        };
                    self.event_bus.publish(EngineEvent::new(
                        "provider.usage",
                        json!({
                            "sessionID": session_id,
                            "correlationID": correlation_ref,
                            "messageID": user_message_id,
                            "promptTokens": prompt_tokens,
                            "completionTokens": completion_tokens,
                            "totalTokens": total_tokens,
                            "usageSource": usage_source,
                        }),
                    ));
                }

                if matches!(requested_tool_mode, ToolMode::Required)
                    && productive_tool_calls_total == 0
                {
                    if requested_write_required
                        && required_write_retry_count > 0
                        && productive_write_tool_calls_total == 0
                        && !is_write_invalid_args_failure_kind(latest_required_tool_failure_kind)
                    {
                        latest_required_tool_failure_kind =
                            RequiredToolFailureKind::WriteRequiredNotSatisfied;
                    }
                    if requested_write_required
                        && required_write_retry_count + 1 < strict_write_retry_max_attempts
                    {
                        required_write_retry_count += 1;
                        followup_context = Some(build_write_required_retry_context(
                            &offered_tool_preview,
                            latest_required_tool_failure_kind,
                            &text,
                            &requested_prewrite_requirements,
                            productive_workspace_inspection_total > 0,
                            productive_concrete_read_total > 0,
                            productive_web_research_total > 0,
                            successful_web_research_total > 0,
                        ));
                        continue;
                    }
                    let progress_made_in_cycle = productive_workspace_inspection_total > 0
                        || productive_concrete_read_total > 0
                        || productive_web_research_total > 0
                        || successful_web_research_total > 0;
                    if should_retry_nonproductive_required_tool_cycle(
                        requested_write_required,
                        false,
                        progress_made_in_cycle,
                        required_tool_retry_count,
                    ) {
                        required_tool_retry_count += 1;
                        followup_context = Some(build_required_tool_retry_context_for_task(
                            &offered_tool_preview,
                            latest_required_tool_failure_kind,
                            &text,
                        ));
                        continue;
                    }
                    completion = required_tool_mode_unsatisfied_completion(
                        latest_required_tool_failure_kind,
                    );
                    if !required_tool_unsatisfied_emitted {
                        required_tool_unsatisfied_emitted = true;
                        self.event_bus.publish(EngineEvent::new(
                            "tool.mode.required.unsatisfied",
                            json!({
                                "sessionID": session_id,
                                "messageID": user_message_id,
                                "iteration": iteration,
                                "selectedToolCount": allowed_tool_names.len(),
                                "offeredToolsPreview": offered_tool_preview,
                                "reason": latest_required_tool_failure_kind.code(),
                            }),
                        ));
                    }
                    self.event_bus.publish(EngineEvent::new(
                        "provider.call.iteration.finish",
                        json!({
                            "sessionID": session_id,
                            "messageID": user_message_id,
                            "iteration": iteration,
                            "finishReason": "required_tool_unsatisfied",
                            "acceptedToolCalls": accepted_tool_calls_in_cycle,
                            "rejectedToolCalls": 0,
                            "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                        }),
                    ));
                } else {
                    if completion.trim().is_empty()
                        && !last_tool_outputs.is_empty()
                        && requested_write_required
                        && empty_completion_retry_count == 0
                    {
                        empty_completion_retry_count += 1;
                        followup_context = Some(build_empty_completion_retry_context(
                            &offered_tool_preview,
                            &text,
                            &requested_prewrite_requirements,
                            productive_workspace_inspection_total > 0,
                            productive_concrete_read_total > 0,
                            productive_web_research_total > 0,
                            successful_web_research_total > 0,
                        ));
                        self.event_bus.publish(EngineEvent::new(
                            "provider.call.iteration.finish",
                            json!({
                                "sessionID": session_id,
                                "messageID": user_message_id,
                                "iteration": iteration,
                                "finishReason": "empty_completion_retry",
                                "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                "rejectedToolCalls": 0,
                            }),
                        ));
                        continue;
                    }
                    let prewrite_gate = evaluate_prewrite_gate(
                        requested_write_required,
                        &requested_prewrite_requirements,
                        PrewriteProgress {
                            productive_write_tool_calls_total,
                            productive_workspace_inspection_total,
                            productive_concrete_read_total,
                            productive_web_research_total,
                            successful_web_research_total,
                            required_write_retry_count,
                            unmet_prewrite_repair_retry_count,
                            prewrite_gate_waived,
                        },
                    );
                    if should_start_prewrite_repair_before_first_write(
                        requested_prewrite_requirements.repair_on_unmet_requirements,
                        productive_write_tool_calls_total,
                        prewrite_gate.prewrite_satisfied,
                        code_workflow_requested,
                    ) && !prewrite_gate_waived
                    {
                        let unmet_prewrite_codes = prewrite_gate.unmet_codes.clone();
                        if unmet_prewrite_repair_retry_count < prewrite_repair_retry_max_attempts()
                        {
                            unmet_prewrite_repair_retry_count += 1;
                            let repair_attempt = unmet_prewrite_repair_retry_count;
                            let repair_attempts_remaining =
                                prewrite_repair_retry_max_attempts().saturating_sub(repair_attempt);
                            followup_context = Some(build_prewrite_repair_retry_context(
                                &offered_tool_preview,
                                latest_required_tool_failure_kind,
                                &text,
                                &requested_prewrite_requirements,
                                productive_workspace_inspection_total > 0,
                                productive_concrete_read_total > 0,
                                productive_web_research_total > 0,
                                successful_web_research_total > 0,
                            ));
                            self.event_bus.publish(EngineEvent::new(
                                "provider.call.iteration.finish",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "finishReason": "prewrite_repair_retry",
                                    "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                    "rejectedToolCalls": 0,
                                    "requiredToolFailureReason": latest_required_tool_failure_kind.code(),
                                    "repair": prewrite_repair_event_payload(
                                        repair_attempt,
                                        repair_attempts_remaining,
                                        &unmet_prewrite_codes,
                                        false,
                                    ),
                                }),
                            ));
                            continue;
                        }
                        if prewrite_gate_strict_mode() {
                            self.event_bus.publish(EngineEvent::new(
                                "prewrite.gate.strict_mode.blocked",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "unmetCodes": unmet_prewrite_codes,
                                }),
                            ));
                            continue;
                        }
                        prewrite_gate_waived = true;
                        let repair_attempt = unmet_prewrite_repair_retry_count;
                        let repair_attempts_remaining =
                            prewrite_repair_retry_max_attempts().saturating_sub(repair_attempt);
                        followup_context = Some(build_prewrite_waived_write_context(
                            &text,
                            &unmet_prewrite_codes,
                        ));
                        self.event_bus.publish(EngineEvent::new(
                            "prewrite.gate.waived.write_executed",
                            json!({
                                "sessionID": session_id,
                                "messageID": user_message_id,
                                "unmetCodes": unmet_prewrite_codes,
                            }),
                        ));
                        self.event_bus.publish(EngineEvent::new(
                            "provider.call.iteration.finish",
                            json!({
                                "sessionID": session_id,
                                "messageID": user_message_id,
                                "iteration": iteration,
                                "finishReason": "prewrite_gate_waived",
                                "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                "rejectedToolCalls": 0,
                                "prewriteGateWaived": true,
                                "repair": prewrite_repair_event_payload(
                                    repair_attempt,
                                    repair_attempts_remaining,
                                    &unmet_prewrite_codes,
                                    true,
                                ),
                            }),
                        ));
                        continue;
                    }
                    if prewrite_gate_waived
                        && requested_write_required
                        && productive_write_tool_calls_total == 0
                        && required_write_retry_count + 1 < strict_write_retry_max_attempts
                    {
                        required_write_retry_count += 1;
                        followup_context = Some(build_write_required_retry_context(
                            &offered_tool_preview,
                            latest_required_tool_failure_kind,
                            &text,
                            &requested_prewrite_requirements,
                            productive_workspace_inspection_total > 0,
                            productive_concrete_read_total > 0,
                            productive_web_research_total > 0,
                            successful_web_research_total > 0,
                        ));
                        self.event_bus.publish(EngineEvent::new(
                            "provider.call.iteration.finish",
                            json!({
                                "sessionID": session_id,
                                "messageID": user_message_id,
                                "iteration": iteration,
                                "finishReason": "waived_write_retry",
                                "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                "rejectedToolCalls": 0,
                            }),
                        ));
                        continue;
                    }
                    self.event_bus.publish(EngineEvent::new(
                        "provider.call.iteration.finish",
                        json!({
                            "sessionID": session_id,
                            "messageID": user_message_id,
                            "iteration": iteration,
                            "finishReason": "provider_completion",
                            "acceptedToolCalls": accepted_tool_calls_in_cycle,
                            "rejectedToolCalls": 0,
                        }),
                    ));
                }
                break;
            }
            if matches!(requested_tool_mode, ToolMode::Required) && productive_tool_calls_total == 0
            {
                completion =
                    required_tool_mode_unsatisfied_completion(latest_required_tool_failure_kind);
                if !required_tool_unsatisfied_emitted {
                    self.event_bus.publish(EngineEvent::new(
                        "tool.mode.required.unsatisfied",
                        json!({
                            "sessionID": session_id,
                            "messageID": user_message_id,
                            "selectedToolCount": tool_call_counts.len(),
                            "reason": latest_required_tool_failure_kind.code(),
                        }),
                    ));
                }
            }
            if completion.trim().is_empty()
                && !last_tool_outputs.is_empty()
                && requested_write_required
                && productive_write_tool_calls_total > 0
            {
                let final_prewrite_satisfied = evaluate_prewrite_gate(
                    requested_write_required,
                    &requested_prewrite_requirements,
                    PrewriteProgress {
                        productive_write_tool_calls_total,
                        productive_workspace_inspection_total,
                        productive_concrete_read_total,
                        productive_web_research_total,
                        successful_web_research_total,
                        required_write_retry_count,
                        unmet_prewrite_repair_retry_count,
                        prewrite_gate_waived,
                    },
                )
                .prewrite_satisfied;
                completion = synthesize_artifact_write_completion_from_tool_state(
                    &text,
                    final_prewrite_satisfied,
                    prewrite_gate_waived,
                );
            }
            if completion.trim().is_empty()
                && !last_tool_outputs.is_empty()
                && should_generate_post_tool_final_narrative(
                    requested_tool_mode,
                    productive_tool_calls_total,
                )
            {
                if let Some(narrative) = self
                    .generate_final_narrative_without_tools(
                        &session_id,
                        &active_agent,
                        Some(provider_id.as_str()),
                        Some(model_id_value.as_str()),
                        cancel.clone(),
                        &last_tool_outputs,
                    )
                    .await
                {
                    completion = narrative;
                }
            }
            if completion.trim().is_empty() && !last_tool_outputs.is_empty() {
                if let Some(summary) = summarize_auth_pending_outputs(&last_tool_outputs) {
                    completion = summary;
                } else if let Some(hint) =
                    summarize_terminal_tool_failure_for_user(&last_tool_outputs)
                {
                    completion = hint;
                } else {
                    let preview = summarize_user_visible_tool_outputs(&last_tool_outputs);
                    if preview.trim().is_empty() {
                        completion = "I used tools for this request, but I couldn't turn the results into a clean final answer. Please retry with the docs page URL, docs path, or exact search query you want me to use.".to_string();
                    } else {
                        completion = format!(
                            "I completed project analysis steps using tools, but the model returned no final narrative text.\n\nTool result summary:\n{}",
                            preview
                        );
                    }
                }
            }
            if completion.trim().is_empty() {
                completion =
                    "I couldn't produce a final response for that run. Please retry your request."
                        .to_string();
            }
            // M-3: Gate fires when email was requested AND email-action tools were
            // actually offered to the agent during at least one iteration but no
            // email action tool was executed. The completion text is NOT consulted —
            // this prevents the model from bypassing the gate by rephrasing, and
            // prevents false positives on legitimate text containing email keywords.
            // Skipping when no email tools were ever offered avoids clobbering
            // legitimate output with a delivery-failure message the agent could not
            // have avoided (e.g. prompts that mention gmail tool names as context).
            if email_delivery_requested && email_tools_ever_offered && !email_action_executed {
                let mut fallback = "I could not verify that an email was sent in this run. I did not complete the delivery action."
                    .to_string();
                if let Some(note) = latest_email_action_note.as_ref() {
                    fallback.push_str("\n\nLast email tool status: ");
                    fallback.push_str(note);
                }
                fallback.push_str(
                    "\n\nPlease retry with an explicit available email tool (for example a draft, reply, or send MCP tool in your current connector set).",
                );
                completion = fallback;
            }
            completion = strip_model_control_markers(&completion);
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
                provider_id: Some(provider_id.as_str()),
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
            let todos_after_fallback = self.storage.get_todos(&session_id).await;
            if todos_after_fallback.is_empty() && !question_tool_used {
                emit_plan_question_fallback(
                    self.storage.clone(),
                    &self.event_bus,
                    &session_id,
                    &user_message_id,
                    &completion,
                )
                .await;
            }
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
}
