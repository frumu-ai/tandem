{
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
                            if is_batch_wrapper_tool_name(&tool_key)
                                && args.as_object().is_none_or(|value| value.is_empty())
                            {
                                continue;
                            }
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
                        if should_block_connector_action_before_concrete_read(
                            &text,
                            &tool_key,
                            productive_concrete_read_total,
                        ) {
                            rejected_tool_call_in_cycle = true;
                            outputs.push(format!(
                                "Tool `{}` call skipped: read the concrete source file listed in this node before connector action tools.",
                                tool_key
                            ));
                            continue;
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
                        let tool_output_result = self
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
                            .await;
                        let Some(output) = (match tool_output_result {
                            Ok(output) => output,
                            Err(err) => {
                                self.mark_session_run_failed(&session_id, &err.to_string())
                                    .await;
                                return Err(err);
                            }
                        }) else {
                            continue;
                        };
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
                                    followup_context = Some(append_recent_tool_results_context(
                                        build_write_required_retry_context(
                                            &offered_tool_preview,
                                            latest_required_tool_failure_kind,
                                            &text,
                                            &requested_prewrite_requirements,
                                            productive_workspace_inspection_total > 0,
                                            productive_concrete_read_total > 0,
                                            productive_web_research_total > 0,
                                            successful_web_research_total > 0,
                                        ),
                                        &last_tool_outputs,
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
                                    continue_prompt_iteration!('prompt_iteration_loop);
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
                                continue_prompt_iteration!('prompt_iteration_loop);
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
                                if unmet_prewrite_repair_retry_count < prewrite_repair_budget {
                                    unmet_prewrite_repair_retry_count += 1;
                                    let repair_attempt = unmet_prewrite_repair_retry_count;
                                    let repair_attempts_remaining =
                                        prewrite_repair_budget.saturating_sub(repair_attempt);
                                    followup_context = Some(append_recent_tool_results_context(
                                        build_prewrite_repair_retry_context(
                                            &offered_tool_preview,
                                            latest_required_tool_failure_kind,
                                            &text,
                                            &requested_prewrite_requirements,
                                            productive_workspace_inspection_total > 0,
                                            productive_concrete_read_total > 0,
                                            productive_web_research_total > 0,
                                            successful_web_research_total > 0,
                                        ),
                                        &last_tool_outputs,
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
                                    continue_prompt_iteration!('prompt_iteration_loop);
                                }
                                if !prewrite_gate_waived {
                                    if prewrite_fail_closed {
                                        let repair_attempt = unmet_prewrite_repair_retry_count;
                                        let repair_attempts_remaining =
                                            prewrite_repair_budget.saturating_sub(repair_attempt);
                                        completion = prewrite_requirements_exhausted_completion(
                                            &unmet_prewrite_codes,
                                            repair_attempt,
                                            repair_attempts_remaining,
                                        );
                                        self.event_bus.publish(EngineEvent::new(
                                            "prewrite.gate.strict_mode.blocked",
                                            json!({
                                                "sessionID": session_id,
                                                "messageID": user_message_id,
                                                "iteration": iteration,
                                                "unmetCodes": unmet_prewrite_codes,
                                            }),
                                        ));
                                        self.event_bus.publish(EngineEvent::new(
                                            "provider.call.iteration.finish",
                                            json!({
                                                "sessionID": session_id,
                                                "messageID": user_message_id,
                                                "iteration": iteration,
                                                "finishReason": "prewrite_requirements_exhausted",
                                                "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                                "rejectedToolCalls": 0,
                                                "requiredToolFailureReason": RequiredToolFailureKind::PrewriteRequirementsExhausted.code(),
                                                "repair": prewrite_repair_event_payload(
                                                    repair_attempt,
                                                    repair_attempts_remaining,
                                                    &unmet_prewrite_codes,
                                                    true,
                                                ),
                                            }),
                                        ));
                                        break;
                                    }
                                    prewrite_gate_waived = true;
                                    let repair_attempt = unmet_prewrite_repair_retry_count;
                                    let repair_attempts_remaining =
                                        prewrite_repair_budget.saturating_sub(repair_attempt);
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
                                    continue_prompt_iteration!('prompt_iteration_loop);
                                }
                            }
                            latest_required_tool_failure_kind =
                                RequiredToolFailureKind::WriteRequiredNotSatisfied;
                            if required_write_retry_count + 1 < strict_write_retry_max_attempts {
                                required_write_retry_count += 1;
                                followup_context = Some(append_recent_tool_results_context(
                                    build_write_required_retry_context(
                                        &offered_tool_preview,
                                        latest_required_tool_failure_kind,
                                        &text,
                                        &requested_prewrite_requirements,
                                        productive_workspace_inspection_total > 0,
                                        productive_concrete_read_total > 0,
                                        productive_web_research_total > 0,
                                        successful_web_research_total > 0,
                                    ),
                                    &last_tool_outputs,
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
                                continue_prompt_iteration!('prompt_iteration_loop);
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
                                continue_prompt_iteration!('prompt_iteration_loop);
                            }
                        }
                        let guard_budget_hit =
                            outputs.iter().any(|o| is_guard_budget_tool_output(o));
                        if (guard_budget_hit || duplicate_signature_hit_in_cycle)
                            && structured_handoff_final_response_requested
                            && !structured_handoff_loop_guard_retry_attempted
                        {
                            structured_handoff_loop_guard_retry_attempted = true;
                            force_structured_handoff_final_response = true;
                            followup_context =
                                Some(structured_handoff_loop_guard_final_retry_context(&outputs));
                            self.event_bus.publish(EngineEvent::new(
                                "provider.call.iteration.finish",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "finishReason": "structured_handoff_loop_guard_final_retry",
                                    "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                    "rejectedToolCalls": 0,
                                }),
                            ));
                            continue_prompt_iteration!('prompt_iteration_loop);
                        }
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
                                && unmet_prewrite_repair_retry_count < prewrite_repair_budget
                                && !prewrite_satisfied
                            {
                                unmet_prewrite_repair_retry_count += 1;
                                let repair_attempt = unmet_prewrite_repair_retry_count;
                                let repair_attempts_remaining =
                                    prewrite_repair_budget.saturating_sub(repair_attempt);
                                followup_context = Some(append_recent_tool_results_context(
                                    build_prewrite_repair_retry_context(
                                        &offered_tool_preview,
                                        latest_required_tool_failure_kind,
                                        &text,
                                        &requested_prewrite_requirements,
                                        productive_workspace_inspection_total > 0,
                                        productive_concrete_read_total > 0,
                                        productive_web_research_total > 0,
                                        successful_web_research_total > 0,
                                    ),
                                    &last_tool_outputs,
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
                                continue_prompt_iteration!('prompt_iteration_loop);
                            }
                            if requested_write_required
                                && productive_write_tool_calls_total > 0
                                && requested_prewrite_requirements.repair_on_unmet_requirements
                                && !prewrite_satisfied
                                && prewrite_fail_closed
                            {
                                let repair_attempt = unmet_prewrite_repair_retry_count;
                                let repair_attempts_remaining =
                                    prewrite_repair_budget.saturating_sub(repair_attempt);
                                completion = prewrite_requirements_exhausted_completion(
                                    &unmet_prewrite_codes,
                                    repair_attempt,
                                    repair_attempts_remaining,
                                );
                                self.event_bus.publish(EngineEvent::new(
                                    "prewrite.gate.strict_mode.blocked",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "unmetCodes": unmet_prewrite_codes,
                                    }),
                                ));
                                self.event_bus.publish(EngineEvent::new(
                                    "provider.call.iteration.finish",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "finishReason": "prewrite_requirements_exhausted",
                                        "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                        "rejectedToolCalls": 0,
                                        "requiredToolFailureReason": RequiredToolFailureKind::PrewriteRequirementsExhausted.code(),
                                        "repair": prewrite_repair_event_payload(
                                            repair_attempt,
                                            repair_attempts_remaining,
                                            &unmet_prewrite_codes,
                                            true,
                                        ),
                                    }),
                                ));
                                break;
                            }
                            if should_complete_after_productive_artifact_write(
                                requested_write_required,
                                productive_write_tool_calls_total,
                                prewrite_satisfied,
                            ) {
                                completion = synthesize_artifact_write_completion_from_tool_state(
                                    &text,
                                    prewrite_satisfied,
                                    prewrite_gate_waived,
                                );
                                self.event_bus.publish(EngineEvent::new(
                                    "provider.call.iteration.finish",
                                    json!({
                                        "sessionID": session_id,
                                        "messageID": user_message_id,
                                        "iteration": iteration,
                                        "finishReason": "artifact_write_completed",
                                        "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                        "rejectedToolCalls": 0,
                                    }),
                                ));
                                break;
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
                            continue_prompt_iteration!('prompt_iteration_loop);
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
                        followup_context = Some(append_recent_tool_results_context(
                            build_write_required_retry_context(
                                &offered_tool_preview,
                                latest_required_tool_failure_kind,
                                &text,
                                &requested_prewrite_requirements,
                                productive_workspace_inspection_total > 0,
                                productive_concrete_read_total > 0,
                                productive_web_research_total > 0,
                                successful_web_research_total > 0,
                            ),
                            &last_tool_outputs,
                        ));
                        continue_prompt_iteration!('prompt_iteration_loop);
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
                        continue_prompt_iteration!('prompt_iteration_loop);
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
                        followup_context = Some(append_recent_tool_results_context(
                            build_empty_completion_retry_context(
                                &offered_tool_preview,
                                &text,
                                &requested_prewrite_requirements,
                                productive_workspace_inspection_total > 0,
                                productive_concrete_read_total > 0,
                                productive_web_research_total > 0,
                                successful_web_research_total > 0,
                            ),
                            &last_tool_outputs,
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
                        continue_prompt_iteration!('prompt_iteration_loop);
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
                        if unmet_prewrite_repair_retry_count < prewrite_repair_budget {
                            unmet_prewrite_repair_retry_count += 1;
                            let repair_attempt = unmet_prewrite_repair_retry_count;
                            let repair_attempts_remaining =
                                prewrite_repair_budget.saturating_sub(repair_attempt);
                            followup_context = Some(append_recent_tool_results_context(
                                build_prewrite_repair_retry_context(
                                    &offered_tool_preview,
                                    latest_required_tool_failure_kind,
                                    &text,
                                    &requested_prewrite_requirements,
                                    productive_workspace_inspection_total > 0,
                                    productive_concrete_read_total > 0,
                                    productive_web_research_total > 0,
                                    successful_web_research_total > 0,
                                ),
                                &last_tool_outputs,
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
                            continue_prompt_iteration!('prompt_iteration_loop);
                        }
                        if prewrite_fail_closed {
                            let repair_attempt = unmet_prewrite_repair_retry_count;
                            let repair_attempts_remaining =
                                prewrite_repair_budget.saturating_sub(repair_attempt);
                            completion = prewrite_requirements_exhausted_completion(
                                &unmet_prewrite_codes,
                                repair_attempt,
                                repair_attempts_remaining,
                            );
                            self.event_bus.publish(EngineEvent::new(
                                "prewrite.gate.strict_mode.blocked",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "unmetCodes": unmet_prewrite_codes,
                                }),
                            ));
                            self.event_bus.publish(EngineEvent::new(
                                "provider.call.iteration.finish",
                                json!({
                                    "sessionID": session_id,
                                    "messageID": user_message_id,
                                    "iteration": iteration,
                                    "finishReason": "prewrite_requirements_exhausted",
                                    "acceptedToolCalls": accepted_tool_calls_in_cycle,
                                    "rejectedToolCalls": 0,
                                    "requiredToolFailureReason": RequiredToolFailureKind::PrewriteRequirementsExhausted.code(),
                                    "repair": prewrite_repair_event_payload(
                                        repair_attempt,
                                        repair_attempts_remaining,
                                        &unmet_prewrite_codes,
                                        true,
                                    ),
                                }),
                            ));
                            break;
                        }
                        prewrite_gate_waived = true;
                        let repair_attempt = unmet_prewrite_repair_retry_count;
                        let repair_attempts_remaining =
                            prewrite_repair_budget.saturating_sub(repair_attempt);
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
                        continue_prompt_iteration!('prompt_iteration_loop);
                    }
                    if prewrite_gate_waived
                        && requested_write_required
                        && productive_write_tool_calls_total == 0
                        && required_write_retry_count + 1 < strict_write_retry_max_attempts
                    {
                        required_write_retry_count += 1;
                        followup_context = Some(append_recent_tool_results_context(
                            build_write_required_retry_context(
                                &offered_tool_preview,
                                latest_required_tool_failure_kind,
                                &text,
                                &requested_prewrite_requirements,
                                productive_workspace_inspection_total > 0,
                                productive_concrete_read_total > 0,
                                productive_web_research_total > 0,
                                successful_web_research_total > 0,
                            ),
                            &last_tool_outputs,
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
                        continue_prompt_iteration!('prompt_iteration_loop);
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
}
