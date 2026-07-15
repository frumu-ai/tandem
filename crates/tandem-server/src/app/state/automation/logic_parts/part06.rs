// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn tool_telemetry_u32(tool_telemetry: &Value, key: &str) -> Option<u32> {
    tool_telemetry
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn automation_push_unique_url(out: &mut Vec<String>, raw: &str) {
    let trimmed = raw
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        })
        .trim_end_matches(['.', ':', '!', '?'])
        .trim();
    if (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        && !out.iter().any(|existing| existing == trimmed)
    {
        out.push(trimmed.to_string());
    }
}

fn automation_collect_urls_from_text(out: &mut Vec<String>, text: &str) {
    for token in text.split_whitespace() {
        automation_push_unique_url(out, token);
    }
}

fn automation_collect_urls_from_value(out: &mut Vec<String>, value: &Value) {
    match value {
        Value::String(text) => automation_collect_urls_from_text(out, text),
        Value::Array(rows) => {
            for row in rows {
                automation_collect_urls_from_value(out, row);
            }
        }
        Value::Object(map) => {
            for (key, row) in map {
                if matches!(
                    key.as_str(),
                    "url" | "final_url" | "canonical" | "source_url" | "href" | "link"
                ) {
                    if let Some(text) = row.as_str() {
                        automation_push_unique_url(out, text);
                    }
                }
                automation_collect_urls_from_value(out, row);
            }
        }
        _ => {}
    }
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
    let mut web_research_citations = Vec::<String>::new();
    let mut latest_web_research_failure = None::<String>;
    let mut email_delivery_attempted = false;
    let mut email_delivery_succeeded = false;
    let mut latest_email_delivery_failure = None::<String>;
    let mut external_mutation_attempted = false;
    let mut external_mutation_succeeded = false;
    let mut latest_external_mutation_failure = None::<String>;
    let mut failed_tools = Vec::<String>::new();
    let mut latest_tool_failure = None::<Value>;
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
            let is_workspace_tool = automation_tool_name_is_workspace_inspection(&normalized);
            let is_web_tool = matches!(
                normalized.as_str(),
                "websearch" | "webfetch" | "webfetch_html"
            );
            let is_email_tool = automation_tool_name_is_email_delivery(&normalized);
            let is_external_mutation_tool = automation_tool_name_is_external_mutation(&normalized);
            let result_failure = automation_tool_result_failure_reason(result.as_ref());
            if error.as_ref().is_some_and(|value| !value.trim().is_empty())
                || result_failure.is_some()
            {
                if !executed_tools.iter().any(|entry| entry == &normalized) {
                    executed_tools.push(normalized.clone());
                }
                if !failed_tools.iter().any(|entry| entry == &normalized) {
                    failed_tools.push(normalized.clone());
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
                        .map(normalize_web_research_failure_label)
                        .or_else(|| {
                            result_failure
                                .as_deref()
                                .map(normalize_web_research_failure_label)
                        });
                }
                if is_email_tool {
                    email_delivery_attempted = true;
                    latest_email_delivery_failure = error
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .or_else(|| result_failure.clone());
                }
                if is_external_mutation_tool {
                    external_mutation_attempted = true;
                    latest_external_mutation_failure = error
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .or_else(|| result_failure.clone());
                }
                latest_tool_failure = Some(json!({
                    "tool": normalized,
                    "reason": error
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .or(result_failure),
                }));
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
                let output_raw =
                    automation_tool_result_output_text(result.as_ref()).unwrap_or_default();
                automation_collect_urls_from_value(&mut web_research_citations, &metadata);
                if let Some(payload) = output_payload.as_ref() {
                    automation_collect_urls_from_value(&mut web_research_citations, payload);
                }
                automation_collect_urls_from_text(&mut web_research_citations, &output_raw);
                let output = output_raw.trim().to_ascii_lowercase();
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
            if is_external_mutation_tool {
                external_mutation_attempted = true;
                external_mutation_succeeded = true;
                latest_external_mutation_failure = None;
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
                     web_research_citations: &mut Vec<String>,
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
                        if automation_tool_name_is_workspace_inspection(&normalized) {
                            *workspace_inspection_used = true;
                        }
                        if matches!(
                            normalized.as_str(),
                            "websearch" | "webfetch" | "webfetch_html"
                        ) {
                            *web_research_used = true;
                            automation_collect_urls_from_text(web_research_citations, block);
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
                            &mut web_research_citations,
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
                    &mut web_research_citations,
                    &mut latest_web_research_failure,
                );
            }
        }
    }
    web_research_citations.sort();
    web_research_citations.dedup();
    web_research_citations.truncate(20);
    let verification = session_verification_summary(node, session);
    json!({
        "requested_tools": requested_tools,
        "executed_tools": executed_tools,
        "failed_tools": failed_tools,
        "latest_tool_failure": latest_tool_failure,
        "tool_call_counts": counts,
        "workspace_inspection_used": workspace_inspection_used,
        "web_research_used": web_research_used,
        "web_research_succeeded": web_research_succeeded,
        "web_research_citations": web_research_citations,
        "latest_web_research_failure": latest_web_research_failure,
        "email_delivery_attempted": email_delivery_attempted,
        "email_delivery_succeeded": email_delivery_succeeded,
        "latest_email_delivery_failure": latest_email_delivery_failure,
        "external_mutation_attempted": external_mutation_attempted,
        "external_mutation_succeeded": external_mutation_succeeded,
        "latest_external_mutation_failure": latest_external_mutation_failure,
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

fn automation_tool_name_is_workspace_inspection(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "glob"
            | "read"
            | "grep"
            | "search"
            | "codesearch"
            | "ls"
            | "list"
            | "repo.context_bundle"
            | "repo.search"
            | "repo.symbol"
            | "repo.neighbors"
            | "repo.impact"
            | "repo.test_targets"
    )
}

pub(crate) fn automation_attempt_receipt_event_payload(
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
        let result_failure = automation_tool_result_failure_reason(result.as_ref());
        if error.as_ref().is_some_and(|value| !value.trim().is_empty()) || result_failure.is_some()
        {
            let mut failed_payload = event_base;
            if failed_payload.get("error").is_none_or(Value::is_null) {
                if let Some(object) = failed_payload.as_object_mut() {
                    object.insert(
                        "error".to_string(),
                        json!(result_failure.unwrap_or_else(|| {
                            "tool result reported a connector/API error".to_string()
                        })),
                    );
                }
            }
            events.push(AutomationAttemptReceiptEventInput {
                event_type: "tool_failed".to_string(),
                payload: failed_payload,
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

pub(crate) fn session_contains_tool_invocations(session: &Session) -> bool {
    session.messages.iter().any(|message| {
        message
            .parts
            .iter()
            .any(|part| matches!(part, MessagePart::ToolInvocation { .. }))
    })
}

pub(crate) fn session_contains_settled_tool_invocations(session: &Session) -> bool {
    session.messages.iter().any(|message| {
        message.parts.iter().any(|part| {
            let MessagePart::ToolInvocation { result, error, .. } = part else {
                return false;
            };
            result.is_some() || error.as_ref().is_some_and(|value| !value.trim().is_empty())
        })
    })
}
