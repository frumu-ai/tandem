use super::*;

impl EngineLoop {
    pub async fn run_prompt_async_with_context(
        &self,
        session_id: String,
        req: SendMessageRequest,
        correlation_id: Option<String>,
    ) -> anyhow::Result<()> {
        let session_record = self.storage.get_session(&session_id).await;
        let session_model = session_record
            .as_ref()
            .and_then(|session| session.model.clone());
        // Per-prompt sampling overrides the session-level default, field by field.
        let session_sampling = session_record
            .as_ref()
            .map(|session| session.sampling)
            .unwrap_or_default();
        let sampling = req.sampling.resolve_over(session_sampling);
        let strict_tool_context = session_record
            .as_ref()
            .and_then(|session| session.verified_tenant_context.as_ref())
            .and_then(|verified| verified.strict_projection.clone());
        let (provider_id, model_id_value) =
            resolve_model_route(req.model.as_ref(), session_model.as_ref()).ok_or_else(|| {
                anyhow::anyhow!(
                "MODEL_SELECTION_REQUIRED: explicit provider/model is required for this request."
            )
            })?;
        let correlation_ref = correlation_id.as_deref();
        let observability_tenant = session_record
            .as_ref()
            .map(|session| &session.tenant_context);
        let observability_org_id = observability_tenant.map(|tenant| tenant.org_id.as_str());
        let observability_workspace_id =
            observability_tenant.map(|tenant| tenant.workspace_id.as_str());
        let emit_provider_event = |level, event| {
            emit_event_with_tenant(
                level,
                ProcessKind::Engine,
                event,
                observability_org_id,
                observability_workspace_id,
            )
        };
        let model_id = Some(model_id_value.as_str());
        let cancel = self.cancellations.create(&session_id).await;
        emit_provider_event(
            Level::INFO,
            ObservabilityEvent {
                event: "provider.call.start",
                component: "engine.loop",
                org_id: None,
                workspace_id: None,
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
        let prewrite_repair_budget = prewrite_repair_retry_budget(&requested_prewrite_requirements);
        let prewrite_fail_closed = prewrite_gate_strict_mode(&requested_prewrite_requirements);
        let request_tool_allowlist = req
            .tool_allowlist
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|tool| normalize_tool_name(&tool))
            .filter(|tool| !tool.trim().is_empty())
            .collect::<HashSet<_>>();
        let required_mcp_tools_before_write =
            concrete_mcp_tools_required_before_write(&request_tool_allowlist);
        let required_mcp_source_wildcards_before_write =
            mcp_source_wildcards_required_before_write(&request_tool_allowlist);
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
                match self
                    .execute_tool_with_permission(
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
                    .await
                {
                    Ok(output) => output.unwrap_or_default(),
                    Err(err) => {
                        self.mark_session_run_failed(&session_id, &err.to_string())
                            .await;
                        return Err(err);
                    }
                }
            }
        } else {
            let mut completion = String::new();
            let max_iteration_budget = max_tool_iterations();
            let mut max_iterations = max_iteration_budget;
            let mut iteration_budget_exhausted = false;
            let mut followup_context: Option<String> = None;
            let mut last_tool_outputs: Vec<String> = Vec::new();
            let mut tool_call_counts: HashMap<String, usize> = HashMap::new();
            let mut productive_tool_call_counts: HashMap<String, usize> = HashMap::new();
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
            let mut productive_artifact_write_tool_calls_total = 0usize;
            let mut productive_workspace_inspection_total = 0usize;
            let mut productive_web_research_total = 0usize;
            let mut productive_concrete_read_total = 0usize;
            let mut successful_web_research_total = 0usize;
            let mut required_tool_retry_count = 0usize;
            let mut required_write_retry_count = 0usize;
            let mut unmet_prewrite_repair_retry_count = 0usize;
            let mut empty_completion_retry_count = 0usize;
            let mut force_structured_handoff_final_response = false;
            let mut structured_handoff_loop_guard_retry_attempted = false;
            let mut prewrite_gate_waived = false;
            let mut invalid_tool_args_retry_count = 0usize;
            let strict_write_retry_max_attempts = strict_write_retry_max_attempts();
            let mut required_tool_unsatisfied_emitted = false;
            let mut latest_required_tool_failure_kind = RequiredToolFailureKind::NoToolCallEmitted;
            let email_delivery_requested = requires_email_delivery_prompt(&text);
            let web_research_requested = requires_web_research_prompt(&text);
            let code_workflow_requested = infer_code_workflow_from_text(&text);
            let required_artifact_target_path = infer_required_output_target_path_from_text(&text);
            let structured_handoff_final_response_requested =
                requires_structured_handoff_final_response_prompt(&text);
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

            macro_rules! continue_prompt_iteration {
                ($loop_label:lifetime) => {{
                    if max_iterations == 0 {
                        iteration_budget_exhausted = true;
                        break $loop_label;
                    }
                    continue $loop_label;
                }};
            }

            'prompt_iteration_loop: while max_iterations > 0 && !cancel.is_cancelled() {
                let iteration = max_iteration_budget
                    .saturating_sub(max_iterations)
                    .saturating_add(1);
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
                let loaded_history =
                    load_chat_history(self.storage.clone(), &session_id, context_profile).await;
                let dropped_history_messages = loaded_history.dropped_messages;
                let dropped_history_chars = loaded_history.dropped_chars;
                let pinned_history_messages = loaded_history.pinned_messages;
                let compacted_tool_results = loaded_history.compacted_tool_results;
                let compacted_tool_result_chars = loaded_history.compacted_tool_result_chars;
                let demoted_tool_invocations = loaded_history.demoted_tool_invocations;
                let demoted_tool_invocation_chars = loaded_history.demoted_tool_invocation_chars;
                let mut messages = loaded_history.messages;
                let mut attachment_count = 0usize;
                let mut attachment_chars = 0usize;
                if iteration == 1 && !runtime_attachments.is_empty() {
                    attach_to_last_user_message(&mut messages, &runtime_attachments);
                    attachment_count = runtime_attachments.len();
                    attachment_chars = runtime_attachment_chars(&runtime_attachments);
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
                if iteration == 1 && matches!(context_profile, ChatHistoryProfile::Full) {
                    let correlation_kind = autonomous_correlation_kind(correlation_ref);
                    self.event_bus.publish(EngineEvent::new(
                        "context.mode.full.selected",
                        json!({
                            "sessionID": session_id,
                            "messageID": user_message_id,
                            "correlationID": correlation_ref,
                            "providerID": provider_id,
                            "modelID": model_id_value,
                            "autonomousLike": correlation_kind.is_some(),
                            "correlationKind": correlation_kind,
                            "historyMessageCount": messages.len(),
                            "historyCharCount": history_char_count,
                        }),
                    ));
                }
                let mut system_parts = vec![tandem_runtime_system_prompt(
                    &self.host_runtime_context,
                    &mcp_server_names,
                )];
                if let Some(system) = active_agent.system_prompt.as_ref() {
                    system_parts.push(system.clone());
                }
                let system_content = system_parts.join("\n\n");
                let system_chars = system_content.len();
                messages.insert(
                    0,
                    ChatMessage {
                        role: "system".to_string(),
                        content: system_content,
                        attachments: Vec::new(),
                    },
                );
                let mut followup_chars = 0usize;
                if let Some(extra) = followup_context.take() {
                    followup_chars = extra.len();
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: extra,
                        attachments: Vec::new(),
                    });
                }
                let pre_hook_message_count = messages.len();
                let pre_hook_chars = messages.iter().map(|m| m.content.len()).sum::<usize>();
                let mut hook_stats = PromptContextHookStats::default();
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
                        Ok(Ok(result)) => {
                            messages = result.messages;
                            hook_stats = result.stats;
                            // TAN-397: audit-only boundary scan of what the
                            // prompt context hook injected (memory/docs/KB/
                            // identity blocks appended beyond the pre-hook
                            // message count). Observational only.
                            if messages.len() > pre_hook_message_count {
                                let injected = messages[pre_hook_message_count..]
                                    .iter()
                                    .map(|message| message.content.as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if let Some(boundary_event) =
                                    data_boundary_gate::evaluate_context_source(
                                        &session_id,
                                        "prompt_context_hook",
                                        None,
                                        &injected,
                                        tandem_data_boundary::DataBoundaryOperationKind::ContextAssembly,
                                    )
                                {
                                    self.event_bus.publish(boundary_event);
                                }
                            }
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
                let hook_added_messages = messages.len().saturating_sub(pre_hook_message_count);
                let hook_added_chars = messages
                    .iter()
                    .map(|m| m.content.len())
                    .sum::<usize>()
                    .saturating_sub(pre_hook_chars);
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
                let prewrite_satisfied = prewrite_gate.prewrite_satisfied;
                let prewrite_gate_write = prewrite_gate.gate_write;
                let force_write_only_retry = prewrite_gate.force_write_only_retry;
                let allow_repair_tools = prewrite_gate.allow_repair_tools;
                let mcp_gate_blocked_by_prewrite_repair =
                    prewrite_repair_prerequisites_block_mcp_gate(
                        &requested_prewrite_requirements,
                        prewrite_satisfied,
                    );
                let pending_required_mcp_tools = unattempted_required_mcp_tools(
                    &required_mcp_tools_before_write,
                    &productive_tool_call_counts,
                );
                let required_mcp_tool_pending =
                    !mcp_gate_blocked_by_prewrite_repair && !pending_required_mcp_tools.is_empty();
                let required_mcp_source_available = tool_schemas.iter().any(|schema| {
                    concrete_mcp_tool_matches_wildcard(
                        &schema.name,
                        &required_mcp_source_wildcards_before_write,
                    )
                });
                let required_mcp_source_pending = requested_write_required
                    && !mcp_gate_blocked_by_prewrite_repair
                    && !required_mcp_source_wildcards_before_write.is_empty()
                    && required_mcp_source_available
                    && !has_attempted_concrete_mcp_for_wildcard(
                        &required_mcp_source_wildcards_before_write,
                        &productive_tool_call_counts,
                    );
                if prewrite_gate_write || required_mcp_source_pending {
                    tool_schemas.retain(|schema| !is_workspace_write_tool(&schema.name));
                }
                if required_mcp_source_pending
                    && tool_call_counts.get("mcp_list").copied().unwrap_or(0) > 0
                {
                    tool_schemas.retain(|schema| normalize_tool_name(&schema.name) != "mcp_list");
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
                                productive_workspace_inspection_total > 0,
                            )
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !repair_tools.is_empty() {
                        tool_schemas = repair_tools;
                    }
                }
                if force_write_only_retry
                    && !allow_repair_tools
                    && !required_mcp_tool_pending
                    && !required_mcp_source_pending
                {
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
                            tool_allowed_by_session_policy(
                                &schema.name,
                                &allowed_tools,
                                requested_write_required,
                            )
                        });
                    }
                }
                if let Some(strict_context) = strict_tool_context.as_ref() {
                    let now_ms = Utc::now().timestamp_millis().max(0) as u64;
                    tool_schemas.retain(|schema| {
                        crate::tool_capabilities::tool_schema_visible_to_strict_context(
                            schema,
                            strict_context,
                            now_ms,
                        )
                    });
                }
                if force_structured_handoff_final_response {
                    tool_schemas.clear();
                }
                let mcp_list_already_attempted =
                    tool_call_counts.get("mcp_list").copied().unwrap_or(0) > 0;
                if required_mcp_tool_pending {
                    tool_schemas.retain(|schema| {
                        let normalized = normalize_tool_name(&schema.name);
                        pending_required_mcp_tools.contains(&normalized)
                            || (!mcp_list_already_attempted
                                && !is_workspace_write_tool(&schema.name)
                                && normalized == "mcp_list")
                    });
                } else if required_mcp_source_pending {
                    tool_schemas.retain(|schema| {
                        let normalized = normalize_tool_name(&schema.name);
                        (!mcp_list_already_attempted && normalized == "mcp_list")
                            || concrete_mcp_tool_matches_wildcard(
                                &normalized,
                                &required_mcp_source_wildcards_before_write,
                            )
                    });
                }
                if let Err(validation_err) = validate_tool_schemas(&tool_schemas) {
                    let detail = validation_err.to_string();
                    emit_provider_event(
                        Level::ERROR,
                        ObservabilityEvent {
                            event: "provider.call.error",
                            component: "engine.loop",
                            org_id: None,
                            workspace_id: None,
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
                let tool_schema_chars = if tool_schemas.is_empty() {
                    0usize
                } else {
                    serde_json::to_string(&tool_schemas)
                        .map(|serialized| serialized.len())
                        .unwrap_or(0)
                };
                let estimated_total_chars = estimated_prompt_chars
                    .saturating_add(tool_schema_chars)
                    .saturating_add(attachment_chars);
                let full_context_mode = matches!(context_profile, ChatHistoryProfile::Full);
                let compaction_occurred = dropped_history_messages > 0;
                let hook_injected_items = hook_stats.injected_count();
                let hook_injected_chars = hook_stats.injected_chars();
                let hook_dropped_items = hook_stats.dropped_count();
                let hook_dropped_chars = hook_stats.dropped_chars();
                let hook_deferred_items = hook_stats.deferred_count();
                let hook_deferred_chars = hook_stats.deferred_chars();
                let hook_sources = hook_stats.sources.clone();
                self.event_bus.publish(EngineEvent::new(
                    "context.budget.final",
                    json!({
                        "sessionID": session_id,
                        "messageID": user_message_id,
                        "correlationID": correlation_ref,
                        "providerID": provider_id,
                        "modelID": model_id_value,
                        "iteration": iteration,
                        "contextMode": format_context_mode(&requested_context_mode, context_is_auto_compact),
                        "historyProfile": context_profile.as_str(),
                        "fullContextMode": full_context_mode,
                        "finalMessageCount": messages.len(),
                        "finalMessageChars": estimated_prompt_chars,
                        "estimatedTotalChars": estimated_total_chars,
                        "estimatedPromptTokens": estimate_tokens_from_chars(estimated_total_chars),
                        "toolSchemaCount": tool_schemas.len(),
                        "toolSchemaChars": tool_schema_chars,
                        "attachmentCount": attachment_count,
                        "attachmentChars": attachment_chars,
                        "contribution": {
                            "systemChars": system_chars,
                            "historyChars": history_char_count,
                            "followupChars": followup_chars,
                            "hookAddedMessages": hook_added_messages,
                            "hookAddedChars": hook_added_chars,
                            "hookBudgetChars": hook_stats.budget_chars,
                            "hookBudgetUsedChars": hook_stats.used_chars,
                            "hookBudgetRemainingChars": hook_stats.remaining_chars,
                            "hookInjectedItems": hook_injected_items,
                            "hookInjectedChars": hook_injected_chars,
                            "hookDroppedItems": hook_dropped_items,
                            "hookDroppedChars": hook_dropped_chars,
                            "hookDeferredItems": hook_deferred_items,
                            "hookDeferredChars": hook_deferred_chars,
                            "hookSources": hook_sources,
                            "toolSchemaChars": tool_schema_chars,
                            "attachmentChars": attachment_chars,
                        },
                        "compactionOccurred": compaction_occurred,
                        "droppedHistoryMessages": dropped_history_messages,
                        "droppedHistoryChars": dropped_history_chars,
                        "droppedDueToBudget": compaction_occurred || hook_dropped_items > 0,
                        "deferredDueToBudget": hook_deferred_items > 0,
                        "pinnedHistoryMessages": pinned_history_messages,
                        "toolResultsCompacted": compacted_tool_results,
                        "toolResultCharsSaved": compacted_tool_result_chars,
                        "staleToolInvocationsDemoted": demoted_tool_invocations,
                        "staleToolInvocationCharsSaved": demoted_tool_invocation_chars,
                    }),
                ));
                // TAN-390/TAN-394: data-boundary evaluation of the fully
                // assembled request, after all context assembly and before
                // dispatch. Audit mode observes only; enforce mode (explicit
                // config) can block, transform, or require approval. Blocked
                // dispatches mirror the hard-budget guard's abort path so the
                // session surfaces a clear, audit-safe error.
                match data_boundary_gate::evaluate_dispatch_boundary(
                    &data_boundary_gate::DataBoundaryDispatchContext {
                        session_id: &session_id,
                        message_id: &user_message_id,
                        correlation_id: correlation_ref,
                        provider_id: provider_id.as_str(),
                        model_id: model_id_value.as_str(),
                        org_id: observability_org_id,
                        workspace_id: observability_workspace_id,
                        deployment_id: observability_tenant
                            .and_then(|tenant| tenant.deployment_id.as_deref()),
                    },
                    &messages,
                ) {
                    data_boundary_gate::DataBoundaryDispatchOutcome::Off => {}
                    data_boundary_gate::DataBoundaryDispatchOutcome::Proceed { event } => {
                        self.event_bus.publish(event);
                    }
                    data_boundary_gate::DataBoundaryDispatchOutcome::ProceedTransformed {
                        event,
                        messages: transformed,
                    } => {
                        self.event_bus.publish(event);
                        messages = transformed;
                    }
                    data_boundary_gate::DataBoundaryDispatchOutcome::RequireApproval {
                        event,
                        evidence,
                        denial_reason,
                    } => {
                        self.event_bus.publish(event);
                        let pending = self
                            .permissions
                            .ask_for_session(Some(&session_id), "data_boundary_egress", evidence)
                            .await;
                        let (reply, timed_out) = self
                            .permissions
                            .wait_for_reply_with_timeout(
                                &pending.id,
                                cancel.clone(),
                                Some(Duration::from_millis(permission_wait_timeout_ms() as u64)),
                            )
                            .await;
                        let approved = matches!(
                            reply.as_deref(),
                            Some("once") | Some("always") | Some("allow")
                        );
                        if !approved {
                            // Deny, cancel, and timeout all fail closed: the
                            // raw payload dispatches only on explicit
                            // approval.
                            let detail = format!(
                                "{denial_reason} ({})",
                                if timed_out {
                                    "approval timed out"
                                } else {
                                    "approval denied"
                                }
                            );
                            self.mark_session_run_failed(&session_id, &detail).await;
                            anyhow::bail!("{detail}");
                        }
                    }
                    data_boundary_gate::DataBoundaryDispatchOutcome::Blocked { event, reason } => {
                        self.event_bus.publish(event);
                        self.mark_session_run_failed(&session_id, &reason).await;
                        anyhow::bail!("{reason}");
                    }
                }
                if full_context_mode {
                    let soft_budget_chars = full_context_soft_budget_chars();
                    let hard_budget_chars = full_context_hard_budget_chars();
                    if estimated_total_chars > soft_budget_chars
                        || estimated_total_chars > hard_budget_chars
                    {
                        let mut top_contributors = vec![
                            ("history", history_char_count),
                            ("system", system_chars),
                            ("hookAdded", hook_added_chars),
                            ("toolSchemas", tool_schema_chars),
                            ("followup", followup_chars),
                            ("attachments", attachment_chars),
                        ];
                        top_contributors.sort_by(|a, b| b.1.cmp(&a.1));
                        let top_contributors = top_contributors
                            .into_iter()
                            .filter(|(_, chars)| *chars > 0)
                            .map(|(source, chars)| json!({"source": source, "chars": chars}))
                            .collect::<Vec<_>>();
                        let hard_exceeded = estimated_total_chars > hard_budget_chars;
                        let hard_override = hard_exceeded && full_context_hard_budget_override();
                        self.event_bus.publish(EngineEvent::new(
                            if hard_exceeded {
                                "context.full.budget.exceeded"
                            } else {
                                "context.full.budget.warning"
                            },
                            json!({
                                "sessionID": session_id,
                                "messageID": user_message_id,
                                "correlationID": correlation_ref,
                                "providerID": provider_id,
                                "modelID": model_id_value,
                                "iteration": iteration,
                                "estimatedTotalChars": estimated_total_chars,
                                "softBudgetChars": soft_budget_chars,
                                "hardBudgetChars": hard_budget_chars,
                                "overrideApplied": hard_override,
                                "topContributors": top_contributors,
                            }),
                        ));
                        if hard_exceeded && !hard_override {
                            let detail = format!(
                                "FULL_CONTEXT_HARD_BUDGET_EXCEEDED: estimated prompt size {} chars exceeds hard budget {} chars; set TANDEM_FULL_CONTEXT_HARD_BUDGET_OVERRIDE=1 to send anyway or use a bounded context mode",
                                estimated_total_chars, hard_budget_chars
                            );
                            emit_provider_event(
                                Level::ERROR,
                                ObservabilityEvent {
                                    event: "provider.call.error",
                                    component: "engine.loop",
                                    org_id: None,
                                    workspace_id: None,
                                    correlation_id: correlation_ref,
                                    session_id: Some(&session_id),
                                    run_id: None,
                                    message_id: Some(&user_message_id),
                                    provider_id: Some(provider_id.as_str()),
                                    model_id,
                                    status: Some("failed"),
                                    error_code: Some("FULL_CONTEXT_HARD_BUDGET_EXCEEDED"),
                                    detail: Some(&detail),
                                },
                            );
                            self.mark_session_run_failed(&session_id, &detail).await;
                            anyhow::bail!("{detail}");
                        }
                    }
                }
                let provider_connect_timeout =
                    Duration::from_millis(provider_stream_connect_timeout_ms() as u64);
                let provider_idle_timeout =
                    Duration::from_millis(provider_stream_idle_timeout_ms() as u64);
                let provider_stream_retry_budget = provider_stream_decode_retry_attempts();
                let mut provider_stream_retry_count = 0usize;
                let mut streamed_tool_calls: HashMap<String, StreamedToolCall> = HashMap::new();
                let mut provider_usage: Option<TokenUsage>;
                let mut accepted_tool_calls_in_cycle: usize;
                'provider_stream_attempt: loop {
                    completion.clear();
                    streamed_tool_calls.clear();
                    provider_usage = None;
                    accepted_tool_calls_in_cycle = 0;
                    let stream_result = tokio::time::timeout(
                        provider_connect_timeout,
                        self.providers.stream_for_provider(
                            Some(provider_id.as_str()),
                            Some(model_id_value.as_str()),
                            messages.clone(),
                            provider_tool_mode_for_selected_tools(
                                &requested_tool_mode,
                                tool_schemas.len(),
                            ),
                            if tool_schemas.is_empty() {
                                None
                            } else {
                                Some(tool_schemas.clone())
                            },
                            sampling,
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
                            if is_transient_provider_stream_error(&error_text)
                                && provider_stream_retry_count < provider_stream_retry_budget
                            {
                                provider_stream_retry_count =
                                    provider_stream_retry_count.saturating_add(1);
                                let detail = truncate_text(&error_text, 500);
                                self.event_bus.publish(EngineEvent::new(
                                    "provider.call.iteration.retry",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "providerID": provider_id,
                                        "modelID": model_id_value,
                                        "iteration": iteration,
                                        "error": detail,
                                        "retry": provider_stream_retry_count,
                                        "maxRetries": provider_stream_retry_budget,
                                    }),
                                ));
                                tokio::time::sleep(provider_stream_retry_backoff_duration(
                                    provider_stream_retry_count,
                                ))
                                .await;
                                continue 'provider_stream_attempt;
                            }
                            let error_code = provider_error_code(&error_text);
                            let detail = truncate_text(&error_text, 500);
                            emit_provider_event(
                                Level::ERROR,
                                ObservabilityEvent {
                                    event: "provider.call.error",
                                    component: "engine.loop",
                                    org_id: None,
                                    workspace_id: None,
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
                                    "providerID": provider_id,
                                    "modelID": model_id_value,
                                    "iteration": iteration,
                                    "errorCode": error_code,
                                    "error": detail,
                                }),
                            ));
                            self.mark_session_run_failed(&session_id, &err.to_string())
                                .await;
                            return Err(err);
                        }
                    };
                    tokio::pin!(stream);
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
                                let error_text = err.to_string();
                                if is_transient_provider_stream_error(&error_text)
                                    && provider_stream_retry_count < provider_stream_retry_budget
                                {
                                    provider_stream_retry_count =
                                        provider_stream_retry_count.saturating_add(1);
                                    let detail = truncate_text(&error_text, 500);
                                    self.event_bus.publish(EngineEvent::new(
                                        "provider.call.iteration.retry",
                                        json!({
                                            "sessionID": session_id,
                                            "messageID": user_message_id,
                                            "providerID": provider_id,
                                            "modelID": model_id_value,
                                            "iteration": iteration,
                                            "error": detail,
                                            "retry": provider_stream_retry_count,
                                            "maxRetries": provider_stream_retry_budget,
                                        }),
                                    ));
                                    tokio::time::sleep(provider_stream_retry_backoff_duration(
                                        provider_stream_retry_count,
                                    ))
                                    .await;
                                    continue 'provider_stream_attempt;
                                }
                                self.event_bus.publish(EngineEvent::new(
                                    "provider.call.iteration.error",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "providerID": provider_id,
                                        "modelID": model_id_value,
                                        "iteration": iteration,
                                        "errorCode": provider_error_code(&error_text),
                                        "error": truncate_text(&error_text, 500),
                                    }),
                                ));
                                self.mark_session_run_failed(&session_id, &error_text).await;
                                return Err(err);
                            }
                        };
                        let Some(chunk) = next_chunk else {
                            break 'provider_stream_attempt;
                        };
                        let chunk = match chunk {
                            Ok(chunk) => chunk,
                            Err(err) => {
                                let error_text = err.to_string();
                                let stream_error_text =
                                    format!("provider stream chunk error: {error_text}");
                                if is_transient_provider_stream_error(&stream_error_text)
                                    && provider_stream_retry_count < provider_stream_retry_budget
                                {
                                    provider_stream_retry_count =
                                        provider_stream_retry_count.saturating_add(1);
                                    let detail = truncate_text(&stream_error_text, 500);
                                    self.event_bus.publish(EngineEvent::new(
                                        "provider.call.iteration.retry",
                                        json!({
                                            "sessionID": session_id,
                                            "messageID": user_message_id,
                                            "providerID": provider_id,
                                            "modelID": model_id_value,
                                            "iteration": iteration,
                                            "error": detail,
                                            "retry": provider_stream_retry_count,
                                            "maxRetries": provider_stream_retry_budget,
                                        }),
                                    ));
                                    tokio::time::sleep(provider_stream_retry_backoff_duration(
                                        provider_stream_retry_count,
                                    ))
                                    .await;
                                    continue 'provider_stream_attempt;
                                }
                                let error_code = provider_error_code(&stream_error_text);
                                let detail = truncate_text(&stream_error_text, 500);
                                emit_provider_event(
                                    Level::ERROR,
                                    ObservabilityEvent {
                                        event: "provider.call.error",
                                        component: "engine.loop",
                                        org_id: None,
                                        workspace_id: None,
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
                                        "providerID": provider_id,
                                        "modelID": model_id_value,
                                        "iteration": iteration,
                                        "errorCode": error_code,
                                        "error": detail,
                                    }),
                                ));
                                let err = anyhow::anyhow!("{stream_error_text}");
                                self.mark_session_run_failed(&session_id, &err.to_string())
                                    .await;
                                return Err(err);
                            }
                        };
                        match chunk {
                            StreamChunk::TextDelta(delta) => {
                                let delta = strip_model_control_markers(&delta);
                                if delta.trim().is_empty() {
                                    continue;
                                }
                                if completion.is_empty() {
                                    emit_provider_event(
                                        Level::INFO,
                                        ObservabilityEvent {
                                            event: "provider.call.first_byte",
                                            component: "engine.loop",
                                            org_id: None,
                                            workspace_id: None,
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
                                let delta_part = WireMessagePart::text(
                                    &session_id,
                                    &user_message_id,
                                    delta.clone(),
                                );
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
                                break 'provider_stream_attempt;
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
                            break 'provider_stream_attempt;
                        }
                    }
                }

                let (prompt_tokens, completion_tokens, total_tokens, usage_source) =
                    provider_usage_token_counts(
                        provider_usage.as_ref(),
                        estimated_prompt_chars,
                        completion.len(),
                    );
                if usage_source == "estimated" {
                    tracing::debug!(
                        session_id = %session_id,
                        provider_id = %provider_id,
                        "provider.usage missing from stream; using char-count estimate \
                         (prompt_chars={estimated_prompt_chars} completion_chars={})",
                        completion.len()
                    );
                }
                self.event_bus.publish(EngineEvent::new(
                    "provider.usage",
                    json!({
                        "sessionID": session_id,
                        "correlationID": correlation_ref,
                        "messageID": user_message_id,
                        "providerID": provider_id,
                        "modelID": model_id_value,
                        "promptTokens": prompt_tokens,
                        "completionTokens": completion_tokens,
                        "totalTokens": total_tokens,
                        "usageSource": usage_source,
                    }),
                ));

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
                    continue_prompt_iteration!('prompt_iteration_loop);
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
                include!("prompt_execution_parts/tool_processing.rs");
                break;
            }
            if iteration_budget_exhausted && !cancel.is_cancelled() {
                let detail = format!(
                    "Prompt execution exhausted the configured iteration budget ({max_iteration_budget}) before the model produced a terminal response."
                );
                emit_provider_event(
                    Level::WARN,
                    ObservabilityEvent {
                        event: "provider.call.iteration.budget_exhausted",
                        component: "engine.loop",
                        org_id: None,
                        workspace_id: None,
                        correlation_id: correlation_ref,
                        session_id: Some(&session_id),
                        run_id: None,
                        message_id: Some(&user_message_id),
                        provider_id: Some(provider_id.as_str()),
                        model_id,
                        status: Some("failed"),
                        error_code: Some("iteration_budget_exhausted"),
                        detail: Some(&detail),
                    },
                );
                self.event_bus.publish(EngineEvent::new(
                    "provider.call.iteration.budget_exhausted",
                    json!({
                        "sessionID": session_id,
                        "messageID": user_message_id,
                        "maxIterations": max_iteration_budget,
                        "error": detail,
                    }),
                ));
                self.mark_session_run_failed(&session_id, &detail).await;
                return Err(anyhow::anyhow!(detail));
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
                && productive_artifact_write_tool_calls_total > 0
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
                if prewrite_fail_closed && !final_prewrite_satisfied {
                    let unmet_prewrite_codes = evaluate_prewrite_gate(
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
                    .unmet_codes;
                    completion = prewrite_requirements_exhausted_completion(
                        &unmet_prewrite_codes,
                        unmet_prewrite_repair_retry_count,
                        prewrite_repair_budget.saturating_sub(unmet_prewrite_repair_retry_count),
                    );
                } else {
                    completion = synthesize_artifact_write_completion_from_tool_state(
                        &text,
                        final_prewrite_satisfied,
                        prewrite_gate_waived,
                    );
                }
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
                        sampling,
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
        emit_provider_event(
            Level::INFO,
            ObservabilityEvent {
                event: "provider.call.finish",
                component: "engine.loop",
                org_id: None,
                workspace_id: None,
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

fn provider_stream_retry_backoff_duration(retry_count: usize) -> Duration {
    let retry = u64::try_from(retry_count).unwrap_or(u64::MAX);
    Duration::from_millis(retry.saturating_mul(50).min(500))
}
