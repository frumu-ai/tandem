fn parsed_status_u32(status: Option<&Value>, key: &str) -> Option<u32> {
    status
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn infer_artifact_repair_state(
    parsed_status: Option<&Value>,
    repair_attempted: bool,
    repair_succeeded: bool,
    semantic_block_reason: Option<&str>,
    tool_telemetry: &Value,
) -> (u32, u32, bool) {
    let default_budget = tandem_core::prewrite_repair_retry_max_attempts() as u32;
    let inferred_attempt = tool_telemetry
        .get("tool_call_counts")
        .and_then(|value| value.get("write"))
        .and_then(Value::as_u64)
        .and_then(|count| count.checked_sub(1))
        .map(|count| count.min(default_budget as u64) as u32)
        .unwrap_or(0);
    let repair_attempt = parsed_status_u32(parsed_status, "repairAttempt").unwrap_or_else(|| {
        if repair_attempted {
            inferred_attempt.max(1)
        } else {
            0
        }
    });
    let repair_attempts_remaining = parsed_status_u32(parsed_status, "repairAttemptsRemaining")
        .unwrap_or_else(|| default_budget.saturating_sub(repair_attempt.min(default_budget)));
    let repair_exhausted = parsed_status
        .and_then(|value| value.get("repairExhausted"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            repair_attempted
                && !repair_succeeded
                && semantic_block_reason.is_some()
                && repair_attempt >= default_budget
        });
    (repair_attempt, repair_attempts_remaining, repair_exhausted)
}

pub(crate) fn summarize_automation_tool_activity(
    node: &AutomationFlowNode,
    session: &Session,
    requested_tools: &[String],
) -> Value {
    let mut executed_tools = Vec::new();
    let mut counts = serde_json::Map::new();
    let mut workspace_inspection_used = false;
    let mut web_research_used = false;
    let mut web_research_succeeded = false;
    let mut latest_web_research_failure = None::<String>;
    let mut email_delivery_attempted = false;
    let mut email_delivery_succeeded = false;
    let mut latest_email_delivery_failure = None::<String>;
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                error,
                result,
                ..
            } = part
            else {
                continue;
            };
            let normalized = tool.trim().to_ascii_lowercase().replace('-', "_");
            let is_workspace_tool = matches!(
                normalized.as_str(),
                "glob" | "read" | "grep" | "search" | "codesearch" | "ls" | "list"
            );
            let is_web_tool = matches!(
                normalized.as_str(),
                "websearch" | "webfetch" | "webfetch_html"
            );
            let is_email_tool = automation_tool_name_is_email_delivery(&normalized);
            if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                if !executed_tools.iter().any(|entry| entry == &normalized) {
                    executed_tools.push(normalized.clone());
                }
                let next_count = counts
                    .get(&normalized)
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .saturating_add(1);
                counts.insert(normalized.clone(), json!(next_count));
                if is_workspace_tool {
                    workspace_inspection_used = true;
                }
                if is_web_tool {
                    web_research_used = true;
                }
                if is_web_tool {
                    latest_web_research_failure = error
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(normalize_web_research_failure_label);
                }
                if is_email_tool {
                    email_delivery_attempted = true;
                    latest_email_delivery_failure = error
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string);
                }
                continue;
            }
            if !executed_tools.iter().any(|entry| entry == &normalized) {
                executed_tools.push(normalized.clone());
            }
            let next_count = counts
                .get(&normalized)
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .saturating_add(1);
            counts.insert(normalized.clone(), json!(next_count));
            if is_workspace_tool {
                workspace_inspection_used = true;
            }
            if is_web_tool {
                web_research_used = true;
                let is_websearch = normalized.as_str() == "websearch";
                let metadata = automation_tool_result_metadata(result.as_ref())
                    .cloned()
                    .unwrap_or(Value::Null);
                let output_payload = automation_tool_result_output_payload(result.as_ref());
                let output = automation_tool_result_output_text(result.as_ref())
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase();
                let result_error = metadata
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let result_has_sources = metadata
                    .get("count")
                    .and_then(Value::as_u64)
                    .is_some_and(|count| count > 0)
                    || output_payload.as_ref().is_some_and(|payload| {
                        payload
                            .get("result_count")
                            .and_then(Value::as_u64)
                            .is_some_and(|count| count > 0)
                            || payload
                                .get("results")
                                .and_then(Value::as_array)
                                .is_some_and(|results| !results.is_empty())
                    });
                let explicit_zero_results = output_payload.as_ref().is_some_and(|payload| {
                    payload
                        .get("result_count")
                        .and_then(Value::as_u64)
                        .is_some_and(|count| count == 0)
                        || payload
                            .get("count")
                            .and_then(Value::as_u64)
                            .is_some_and(|count| count == 0)
                        || payload
                            .get("results")
                            .and_then(Value::as_array)
                            .is_some_and(|results| results.is_empty())
                });
                let timed_out = result_error
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case("timeout"))
                    || output.contains("search timed out")
                    || output.contains("no results received")
                    || output.contains("timed out");
                let unavailable = result_error
                    .as_deref()
                    .is_some_and(web_research_unavailable_failure)
                    || web_research_unavailable_failure(&output);
                let meaningful_web_result = if is_websearch {
                    result_has_sources
                        || (!output.is_empty()
                            && !explicit_zero_results
                            && !output.contains("no results")
                            && !output.contains("0 results")
                            && !output.contains("\"result_count\": 0")
                            && !output.contains("\"result_count\":0")
                            && !output.contains("\"count\": 0")
                            && !output.contains("\"count\":0"))
                } else {
                    !output.is_empty()
                };
                if meaningful_web_result && !timed_out && !unavailable {
                    web_research_succeeded = true;
                    latest_web_research_failure = None;
                } else if latest_web_research_failure.is_none() {
                    latest_web_research_failure = result_error
                        .map(|value| normalize_web_research_failure_label(&value))
                        .or_else(|| {
                            if timed_out {
                                Some("web research timed out".to_string())
                            } else if unavailable {
                                Some(normalize_web_research_failure_label(&output))
                            } else if is_websearch && !result_has_sources {
                                Some("web research returned no results".to_string())
                            } else if output.is_empty() {
                                Some("web research returned no usable output".to_string())
                            } else {
                                Some("web research returned an unusable result".to_string())
                            }
                        });
                }
            }
            if is_email_tool {
                email_delivery_attempted = true;
                email_delivery_succeeded = true;
                latest_email_delivery_failure = None;
            }
        }
    }
    if executed_tools.is_empty() {
        for message in &session.messages {
            for part in &message.parts {
                let MessagePart::Text { text } = part else {
                    continue;
                };
                if !text.contains("Tool result summary:") {
                    continue;
                }
                let mut current_tool = None::<String>;
                let mut current_block = String::new();
                let flush_summary_block =
                    |tool_name: &Option<String>,
                     block: &str,
                     executed_tools: &mut Vec<String>,
                     counts: &mut serde_json::Map<String, Value>,
                     workspace_inspection_used: &mut bool,
                     web_research_used: &mut bool,
                     web_research_succeeded: &mut bool,
                     latest_web_research_failure: &mut Option<String>| {
                        let Some(tool_name) = tool_name.as_ref() else {
                            return;
                        };
                        let normalized = tool_name.trim().to_ascii_lowercase().replace('-', "_");
                        if !executed_tools.iter().any(|entry| entry == &normalized) {
                            executed_tools.push(normalized.clone());
                        }
                        let next_count = counts
                            .get(&normalized)
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                            .saturating_add(1);
                        counts.insert(normalized.clone(), json!(next_count));
                        if matches!(
                            normalized.as_str(),
                            "glob" | "read" | "grep" | "search" | "codesearch" | "ls" | "list"
                        ) {
                            *workspace_inspection_used = true;
                        }
                        if matches!(
                            normalized.as_str(),
                            "websearch" | "webfetch" | "webfetch_html"
                        ) {
                            *web_research_used = true;
                            let lowered = block.to_ascii_lowercase();
                            if lowered.contains("timed out")
                                || lowered.contains("no results received")
                            {
                                *latest_web_research_failure =
                                    Some("web research timed out".to_string());
                            } else if web_research_unavailable_failure(&lowered) {
                                *latest_web_research_failure =
                                    Some(normalize_web_research_failure_label(&lowered));
                            } else if !block.trim().is_empty() {
                                *web_research_succeeded = true;
                                *latest_web_research_failure = None;
                            }
                        }
                    };
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("Tool `") && trimmed.ends_with("` result:") {
                        flush_summary_block(
                            &current_tool,
                            &current_block,
                            &mut executed_tools,
                            &mut counts,
                            &mut workspace_inspection_used,
                            &mut web_research_used,
                            &mut web_research_succeeded,
                            &mut latest_web_research_failure,
                        );
                        current_block.clear();
                        current_tool = trimmed
                            .strip_prefix("Tool `")
                            .and_then(|value| value.strip_suffix("` result:"))
                            .map(str::to_string);
                        continue;
                    }
                    if current_tool.is_some() {
                        if !current_block.is_empty() {
                            current_block.push('\n');
                        }
                        current_block.push_str(trimmed);
                    }
                }
                flush_summary_block(
                    &current_tool,
                    &current_block,
                    &mut executed_tools,
                    &mut counts,
                    &mut workspace_inspection_used,
                    &mut web_research_used,
                    &mut web_research_succeeded,
                    &mut latest_web_research_failure,
                );
            }
        }
    }
    let verification = session_verification_summary(node, session);
    json!({
        "requested_tools": requested_tools,
        "executed_tools": executed_tools,
        "tool_call_counts": counts,
        "workspace_inspection_used": workspace_inspection_used,
        "web_research_used": web_research_used,
        "web_research_succeeded": web_research_succeeded,
        "latest_web_research_failure": latest_web_research_failure,
        "email_delivery_attempted": email_delivery_attempted,
        "email_delivery_succeeded": email_delivery_succeeded,
        "latest_email_delivery_failure": latest_email_delivery_failure,
        "verification_expected": verification.get("verification_expected").cloned().unwrap_or(json!(false)),
        "verification_command": verification.get("verification_command").cloned().unwrap_or(Value::Null),
        "verification_plan": verification.get("verification_plan").cloned().unwrap_or(json!([])),
        "verification_results": verification.get("verification_results").cloned().unwrap_or(json!([])),
        "verification_outcome": verification.get("verification_outcome").cloned().unwrap_or(Value::Null),
        "verification_total": verification.get("verification_total").cloned().unwrap_or(json!(0)),
        "verification_completed": verification.get("verification_completed").cloned().unwrap_or(json!(0)),
        "verification_passed_count": verification.get("verification_passed_count").cloned().unwrap_or(json!(0)),
        "verification_failed_count": verification.get("verification_failed_count").cloned().unwrap_or(json!(0)),
        "verification_ran": verification.get("verification_ran").cloned().unwrap_or(json!(false)),
        "verification_failed": verification.get("verification_failed").cloned().unwrap_or(json!(false)),
        "latest_verification_command": verification.get("latest_verification_command").cloned().unwrap_or(Value::Null),
        "latest_verification_failure": verification.get("latest_verification_failure").cloned().unwrap_or(Value::Null),
    })
}

fn automation_attempt_receipt_event_payload(
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    session_id: &str,
    tool: &str,
    call_index: usize,
    args: &Value,
    result: Option<&Value>,
    error: Option<&str>,
) -> Value {
    json!({
        "automation_id": automation.automation_id,
        "run_id": run_id,
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "tool": tool,
        "call_index": call_index,
        "args": args,
        "result": result.cloned().unwrap_or(Value::Null),
        "error": error.map(str::to_string),
    })
}

pub(crate) fn collect_automation_attempt_receipt_events(
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    session_id: &str,
    session: &Session,
    verified_output: Option<&(String, String)>,
    verified_output_resolution: Option<&AutomationVerifiedOutputResolution>,
    required_output_path: Option<&str>,
    artifact_validation: Option<&Value>,
) -> Vec<AutomationAttemptReceiptEventInput> {
    let mut events = Vec::new();
    for (call_index, part) in session
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .enumerate()
    {
        let MessagePart::ToolInvocation {
            tool,
            args,
            result,
            error,
        } = part
        else {
            continue;
        };

        let event_base = automation_attempt_receipt_event_payload(
            automation,
            run_id,
            node,
            attempt,
            session_id,
            tool,
            call_index,
            args,
            result.as_ref(),
            error.as_deref(),
        );
        events.push(AutomationAttemptReceiptEventInput {
            event_type: "tool_invoked".to_string(),
            payload: event_base.clone(),
        });
        if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
            events.push(AutomationAttemptReceiptEventInput {
                event_type: "tool_failed".to_string(),
                payload: event_base,
            });
        } else {
            events.push(AutomationAttemptReceiptEventInput {
                event_type: "tool_succeeded".to_string(),
                payload: event_base,
            });
        }
    }

    if let Some(promoted_from) = verified_output_resolution
        .and_then(|resolution| resolution.legacy_workspace_artifact_promoted_from.as_ref())
    {
        let promoted_to = verified_output_resolution
            .map(|resolution| resolution.path.to_string_lossy().to_string())
            .unwrap_or_default();
        events.push(AutomationAttemptReceiptEventInput {
            event_type: "legacy_workspace_artifact_promoted".to_string(),
            payload: json!({
                "automation_id": automation.automation_id,
                "run_id": run_id,
                "node_id": node.node_id,
                "attempt": attempt,
                "session_id": session_id,
                "promoted_from_path": promoted_from.to_string_lossy().to_string(),
                "promoted_to_path": promoted_to,
            }),
        });
    }

    if let Some((path, text)) = verified_output {
        events.push(AutomationAttemptReceiptEventInput {
            event_type: "artifact_write_success".to_string(),
            payload: json!({
                "automation_id": automation.automation_id,
                "run_id": run_id,
                "node_id": node.node_id,
                "attempt": attempt,
                "session_id": session_id,
                "path": path,
                "content_digest": crate::sha256_hex(&[text]),
                "status": artifact_validation
                    .and_then(|value| value.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("succeeded"),
            }),
        });
    } else if let Some(path) = required_output_path {
        events.push(AutomationAttemptReceiptEventInput {
            event_type: "artifact_write_failed".to_string(),
            payload: json!({
                "automation_id": automation.automation_id,
                "run_id": run_id,
                "node_id": node.node_id,
                "attempt": attempt,
                "session_id": session_id,
                "path": path,
                "status": artifact_validation
                    .and_then(|value| value.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("failed"),
                "reason": artifact_validation
                    .and_then(|value| value.get("semantic_block_reason"))
                    .and_then(Value::as_str)
                    .or_else(|| {
                        artifact_validation
                            .and_then(|value| value.get("rejected_artifact_reason"))
                            .and_then(Value::as_str)
                    }),
                "session_tool_activity": summarize_automation_tool_activity(node, session, &[])
                    .get("tool_call_counts")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            }),
        });
    }

    events
}

async fn load_automation_session_after_run(
    state: &AppState,
    session_id: &str,
    expect_tool_activity: bool,
) -> Option<Session> {
    let mut last = state.storage.get_session(session_id).await?;
    if !expect_tool_activity || session_contains_settled_tool_invocations(&last) {
        return Some(last);
    }

    // `message.part.updated` events are persisted asynchronously. Wait for a
    // settled tool snapshot (result/error), not just a transient invocation.
    let mut saw_any_invocation = session_contains_tool_invocations(&last);
    for attempt in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        let current = state.storage.get_session(session_id).await?;
        if session_contains_settled_tool_invocations(&current) {
            return Some(current);
        }
        saw_any_invocation |= session_contains_tool_invocations(&current);
        last = current;
        if !saw_any_invocation && attempt >= 20 {
            break;
        }
    }
    Some(last)
}

fn session_contains_tool_invocations(session: &Session) -> bool {
    session.messages.iter().any(|message| {
        message
            .parts
            .iter()
            .any(|part| matches!(part, MessagePart::ToolInvocation { .. }))
    })
}

fn session_contains_settled_tool_invocations(session: &Session) -> bool {
    session.messages.iter().any(|message| {
        message.parts.iter().any(|part| {
            let MessagePart::ToolInvocation { result, error, .. } = part else {
                return false;
            };
            result.is_some() || error.as_ref().is_some_and(|value| !value.trim().is_empty())
        })
    })
}

async fn record_automation_external_actions_for_session(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    attempt: u32,
    session_id: &str,
    session: &Session,
) -> anyhow::Result<Vec<ExternalActionRecord>> {
    let actions = collect_automation_external_action_receipts(
        &state.capability_resolver.list_bindings().await?,
        run_id,
        automation,
        node,
        attempt,
        session_id,
        session,
    );
    let mut recorded = Vec::with_capacity(actions.len());
    for action in actions {
        recorded.push(state.record_external_action(action).await?);
    }
    Ok(recorded)
}

pub(crate) fn collect_automation_external_action_receipts(
    bindings: &capability_resolver::CapabilityBindingsFile,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    attempt: u32,
    session_id: &str,
    session: &Session,
) -> Vec<ExternalActionRecord> {
    if !automation_node_is_outbound_action(node) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (call_index, part) in session
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .enumerate()
    {
        let MessagePart::ToolInvocation {
            tool,
            args,
            result,
            error,
        } = part
        else {
            continue;
        };
        if error.as_ref().is_some_and(|value| !value.trim().is_empty()) || result.is_none() {
            continue;
        }
        let Some(binding) = bindings
            .bindings
            .iter()
            .find(|binding| automation_binding_matches_tool_name(binding, tool))
        else {
            continue;
        };
        let idempotency_key = automation_external_action_idempotency_key(
            automation,
            run_id,
            node,
            tool,
            args,
            &call_index.to_string(),
        );
        if !seen.insert(idempotency_key.clone()) {
            continue;
        }
        let source_id = format!("{run_id}:{}:{attempt}:{call_index}", node.node_id);
        let created_at_ms = now_ms();
        out.push(ExternalActionRecord {
            action_id: format!("automation-external-{}", &idempotency_key[..16]),
            operation: binding.capability_id.clone(),
            status: "posted".to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some(source_id),
            routine_run_id: None,
            context_run_id: Some(format!("automation-v2-{run_id}")),
            capability_id: Some(binding.capability_id.clone()),
            provider: Some(binding.provider.clone()),
            target: automation_external_action_target(args, result.as_ref()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some(idempotency_key),
            receipt: Some(json!({
                "tool": tool,
                "args": args,
                "result": result,
            })),
            error: None,
            metadata: Some(json!({
                "automationID": automation.automation_id,
                "automationRunID": run_id,
                "nodeID": node.node_id,
                "attempt": attempt,
                "nodeObjective": node.objective,
                "sessionID": session_id,
                "tool": tool,
                "provider": binding.provider,
            })),
            created_at_ms,
            updated_at_ms: created_at_ms,
        });
    }
    out
}

fn automation_external_action_idempotency_key(
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    tool: &str,
    args: &Value,
    call_index: &str,
) -> String {
    crate::sha256_hex(&[
        "automation_v2",
        &automation.automation_id,
        run_id,
        &node.node_id,
        tool,
        &args.to_string(),
        call_index,
    ])
}

fn automation_attempt_uses_legacy_fallback(
    session_text: &str,
    artifact_validation: Option<&Value>,
) -> bool {
    if artifact_validation
        .and_then(|value| value.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .is_some()
    {
        return false;
    }
    let lowered = session_text
        .chars()
        .take(1600)
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "status: blocked",
        "status blocked",
        "## status blocked",
        "blocked pending",
        "this brief is blocked",
        "brief is blocked",
        "partially blocked",
        "provisional",
        "path-level evidence",
        "based on filenames not content",
        "could not be confirmed from file contents",
        "could not safely cite exact file-derived claims",
        "not approved",
        "approval has not happened",
        "publication is blocked",
        "i’m blocked",
        "i'm blocked",
        "status: verify_failed",
        "status verify_failed",
        "verification failed",
        "tests failed",
        "build failed",
        "lint failed",
        "verify failed",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

pub(crate) fn automation_publish_editorial_block_reason(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
) -> Option<String> {
    if !automation_node_is_outbound_action(node) {
        return None;
    }
    let mut upstream_ids = node.depends_on.clone();
    for input in &node.input_refs {
        if !upstream_ids
            .iter()
            .any(|value| value == &input.from_step_id)
        {
            upstream_ids.push(input.from_step_id.clone());
        }
    }
    let blocked_upstreams = upstream_ids
        .into_iter()
        .filter(|node_id| {
            let Some(output) = run.checkpoint.node_outputs.get(node_id) else {
                return false;
            };
            output
                .get("failure_kind")
                .and_then(Value::as_str)
                .is_some_and(|value| value == "editorial_quality_failed")
                || output
                    .get("phase")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == "editorial_validation")
                || output
                    .get("validator_summary")
                    .and_then(|value| value.get("unmet_requirements"))
                    .and_then(Value::as_array)
                    .is_some_and(|requirements| {
                        requirements.iter().any(|value| {
                            matches!(
                                value.as_str(),
                                Some("editorial_substance_missing")
                                    | Some("markdown_structure_missing")
                                    | Some("editorial_clearance_required")
                            )
                        })
                    })
        })
        .collect::<Vec<_>>();
    if blocked_upstreams.is_empty() {
        None
    } else {
        Some(format!(
            "publish step blocked until upstream editorial issues are resolved: {}",
            blocked_upstreams.join(", ")
        ))
    }
}

fn automation_binding_matches_tool_name(
    binding: &capability_resolver::CapabilityBinding,
    tool_name: &str,
) -> bool {
    binding.tool_name.eq_ignore_ascii_case(tool_name)
        || binding
            .tool_name_aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(tool_name))
}

fn automation_external_action_target(args: &Value, result: Option<&Value>) -> Option<String> {
    for candidate in [
        args.pointer("/owner_repo").and_then(Value::as_str),
        args.pointer("/repo").and_then(Value::as_str),
        args.pointer("/repository").and_then(Value::as_str),
        args.pointer("/channel").and_then(Value::as_str),
        args.pointer("/channel_id").and_then(Value::as_str),
        args.pointer("/thread_ts").and_then(Value::as_str),
        result
            .and_then(|value| value.pointer("/metadata/channel"))
            .and_then(Value::as_str),
        result
            .and_then(|value| value.pointer("/metadata/repo"))
            .and_then(Value::as_str),
    ] {
        let trimmed = candidate.map(str::trim).unwrap_or_default();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub(crate) fn automation_node_max_attempts(node: &AutomationFlowNode) -> u32 {
    let explicit = node
        .retry_policy
        .as_ref()
        .and_then(|value| value.get("max_attempts"))
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 10) as u32);
    if let Some(value) = explicit {
        return value;
    }
    let validator_kind = automation_output_validator_kind(node);
    if validator_kind == crate::AutomationOutputValidatorKind::StandupUpdate {
        return 3;
    }
    if validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief
        || !automation_node_required_tools(node).is_empty()
    {
        5
    } else {
        3
    }
}

pub(crate) fn automation_output_is_blocked(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("blocked"))
}

pub(crate) fn automation_output_is_verify_failed(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("verify_failed"))
}

pub(crate) fn automation_output_needs_repair(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("needs_repair"))
}

pub(crate) fn automation_output_has_warnings(output: &Value) -> bool {
    output
        .get("validator_summary")
        .and_then(|value| value.get("warning_count"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            output
                .get("artifact_validation")
                .and_then(|value| value.get("warning_count"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        })
        > 0
}

pub(crate) fn automation_output_repair_exhausted(output: &Value) -> bool {
    output
        .get("artifact_validation")
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn automation_output_failure_reason(output: &Value) -> Option<String> {
    output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn automation_output_blocked_reason(output: &Value) -> Option<String> {
    output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn automation_output_is_passing(output: &Value) -> bool {
    output
        .get("validator_summary")
        .and_then(|v| v.get("outcome"))
        .and_then(Value::as_str)
        .is_some_and(|outcome| {
            outcome.eq_ignore_ascii_case("passed")
                || outcome.eq_ignore_ascii_case("accepted_with_warnings")
        })
        && output
            .get("validator_summary")
            .and_then(|v| v.get("unmet_requirements"))
            .and_then(Value::as_array)
            .map(|reqs| reqs.is_empty())
            .unwrap_or(false)
}

pub(crate) fn automation_node_has_passing_artifact(
    node_id: &str,
    checkpoint: &crate::automation_v2::types::AutomationRunCheckpoint,
) -> bool {
    checkpoint
        .node_outputs
        .get(node_id)
        .map(automation_output_is_passing)
        .unwrap_or(false)
}

pub(crate) async fn resolve_automation_v2_workspace_root(
    state: &AppState,
    automation: &AutomationV2Spec,
) -> String {
    if let Some(workspace_root) = automation
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        return workspace_root;
    }
    if let Some(workspace_root) = automation
        .metadata
        .as_ref()
        .and_then(|row| row.get("workspace_root"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        return workspace_root;
    }
    state.workspace_index.snapshot().await.root
}

fn automation_declared_output_paths(automation: &AutomationV2Spec) -> Vec<String> {
    let mut paths = Vec::new();
    for target in &automation.output_targets {
        let trimmed = target.trim();
        if !trimmed.is_empty() && !paths.iter().any(|existing| existing == trimmed) {
            paths.push(trimmed.to_string());
        }
    }
    for node in &automation.flow.nodes {
        if let Some(path) = automation_node_required_output_path(node) {
            let trimmed = path.trim();
            if !trimmed.is_empty() && !paths.iter().any(|existing| existing == trimmed) {
                paths.push(trimmed.to_string());
            }
        }
    }
    paths
}
