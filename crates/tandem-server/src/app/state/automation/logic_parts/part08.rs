fn automation_node_metadata_bool_local(node: &AutomationFlowNode, key: &str) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_metadata_string_local(node: &AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn automation_notion_data_source_id(node: &AutomationFlowNode) -> Option<String> {
    automation_node_metadata_string_local(node, "notion_data_source_id").or_else(|| {
        automation_node_metadata_string_local(node, "notion_data_source_url")
            .and_then(|value| value.strip_prefix("collection://").map(str::to_string))
    })
}

fn automation_notion_data_source_url(node: &AutomationFlowNode) -> Option<String> {
    automation_node_metadata_string_local(node, "notion_data_source_url").or_else(|| {
        automation_notion_data_source_id(node).map(|id| format!("collection://{id}"))
    })
}

fn automation_notion_source_node_id(node: &AutomationFlowNode) -> Option<String> {
    automation_node_metadata_string_local(node, "source_node_id")
        .or_else(|| automation_node_metadata_string_local(node, "filter_node_id"))
        .or_else(|| node.input_refs.first().map(|input| input.from_step_id.clone()))
}

fn automation_notion_status_property_value(node: &AutomationFlowNode) -> String {
    automation_node_metadata_string_local(node, "status_property_value")
        .unwrap_or_else(|| "Not started".to_string())
}

fn automation_node_is_notion_connector_writer(node: &AutomationFlowNode) -> bool {
    automation_node_metadata_bool_local(node, "connector_writer")
        && automation_notion_data_source_id(node).is_some()
        && automation_notion_data_source_url(node).is_some()
}

fn automation_parse_json_text(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() || !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    serde_json::from_str::<Value>(trimmed).ok()
}

fn automation_notion_leads_from_candidate(value: &Value) -> Option<Vec<Value>> {
    if let Some(leads) = value.get("leads").and_then(Value::as_array) {
        return Some(leads.clone());
    }
    if let Some(text) = value.as_str() {
        if let Some(parsed) = automation_parse_json_text(text) {
            return automation_notion_leads_from_candidate(&parsed);
        }
    }
    for pointer in [
        "/content/text",
        "/content/artifact",
        "/content/verified_output",
        "/artifact/text",
        "/verified_output/text",
        "/text",
    ] {
        if let Some(candidate) = value.pointer(pointer) {
            if let Some(leads) = automation_notion_leads_from_candidate(candidate) {
                return Some(leads);
            }
        }
    }
    None
}

fn automation_notion_writer_leads(
    node: &AutomationFlowNode,
    upstream_inputs: &[Value],
    source_node_id: &str,
) -> Vec<Value> {
    for input in upstream_inputs {
        let alias_matches = input.get("alias").and_then(Value::as_str) == Some("filtered_leads");
        let source_matches = input
            .get("from_step_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value == source_node_id);
        if !(alias_matches || source_matches) {
            continue;
        }
        if let Some(output) = input.get("output") {
            if let Some(leads) = automation_notion_leads_from_candidate(output) {
                return leads;
            }
        }
    }

    for input in upstream_inputs {
        if let Some(output) = input.get("output") {
            if let Some(leads) = automation_notion_leads_from_candidate(output) {
                return leads;
            }
        }
    }

    if node.input_refs.is_empty() {
        return Vec::new();
    }
    Vec::new()
}

fn automation_notion_string(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn automation_notion_normalize_subreddit(value: &str) -> String {
    let trimmed = value.trim();
    let prefixed = if trimmed.starts_with("r/") {
        trimmed.to_string()
    } else if trimmed.is_empty() {
        String::new()
    } else {
        format!("r/{trimmed}")
    };
    match prefixed.as_str() {
        "r/LocalLLaMA" | "r/sysadmin" | "r/mcp" | "r/DevOps" => prefixed,
        _ => "r/LocalLLaMA".to_string(),
    }
}

fn automation_notion_lead_properties(lead: &Value, status_value: &str) -> Value {
    let title = automation_notion_string(lead, &["topic_thread_title", "Topic / Thread Title", "title"]);
    let subreddit = automation_notion_normalize_subreddit(&automation_notion_string(
        lead,
        &["subreddit", "Subreddit"],
    ));
    let pain = automation_notion_string(lead, &["core_pain_point", "Core Pain Point", "pain_point"]);
    let handle = automation_notion_string(lead, &["user_handle", "User Handle", "author"]);
    let link = automation_notion_string(lead, &["thread_link", "Thread Link", "permalink", "url"]);

    json!({
        "Topic / Thread Title": title,
        "Subreddit": subreddit,
        "Core Pain Point": pain,
        "User Handle": handle,
        "Thread Link": link,
        "Status": status_value,
    })
}

fn automation_tool_output_value(output: &str) -> Value {
    serde_json::from_str::<Value>(output).unwrap_or_else(|_| json!({ "text": output }))
}

fn automation_notion_tool_error(value: &Value) -> Option<String> {
    if value
        .get("success")
        .and_then(Value::as_bool)
        .is_some_and(|success| !success)
    {
        return Some(value.to_string());
    }
    if value
        .get("status")
        .and_then(Value::as_i64)
        .is_some_and(|status| status >= 400)
    {
        return Some(value.to_string());
    }
    if value
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| name.contains("Error"))
    {
        return Some(value.to_string());
    }
    if value
        .get("code")
        .and_then(Value::as_str)
        .is_some_and(|code| code.contains("error") || code == "object_not_found")
    {
        return Some(value.to_string());
    }
    None
}

fn automation_notion_value_contains(value: &Value, needle: &str) -> bool {
    let trimmed = needle.trim();
    !trimmed.is_empty() && value.to_string().contains(trimmed)
}

fn automation_notion_find_page_ref(value: &Value) -> Option<String> {
    fn walk(value: &Value, key_hint: Option<&str>, urls: &mut Vec<String>, ids: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    let lowered = key.to_ascii_lowercase();
                    if lowered.contains("request_id")
                        || lowered == "requestid"
                        || lowered.contains("integration_id")
                        || lowered.contains("data_source")
                    {
                        continue;
                    }
                    walk(child, Some(key), urls, ids);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, key_hint, urls, ids);
                }
            }
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() || trimmed.starts_with("collection://") {
                    return;
                }
                if trimmed.contains("notion.so") || trimmed.contains("notion.site") {
                    urls.push(trimmed.to_string());
                    return;
                }
                let Some(key) = key_hint else {
                    return;
                };
                let lowered = key.to_ascii_lowercase();
                if lowered == "page_id" || lowered == "pageid" || lowered == "id" {
                    ids.push(trimmed.to_string());
                }
            }
            _ => {}
        }
    }

    let mut urls = Vec::new();
    let mut ids = Vec::new();
    walk(value, None, &mut urls, &mut ids);
    urls.into_iter().next().or_else(|| ids.into_iter().next())
}

async fn automation_dispatch_deterministic_tool(
    state: &AppState,
    run_id: &str,
    node: &AutomationFlowNode,
    tenant_context: tandem_types::TenantContext,
    dispatch_scope: &[String],
    tool: &str,
    args: Value,
    invocation_parts: &mut Vec<MessagePart>,
    call_rows: &mut Vec<Value>,
) -> Result<Value, String> {
    let dispatch_context = state.tool_dispatch_context(
        tandem_tools::ToolDispatchSource::new("automation_notion_writer")
            .run(run_id)
            .node(node.node_id.clone()),
        tenant_context,
        dispatch_scope.to_vec(),
    );
    match state
        .tool_dispatcher
        .dispatch(tool, args.clone(), dispatch_context)
        .await
    {
        Ok(result) => {
            let parsed_output = automation_tool_output_value(&result.output);
            let result_value = json!({
                "output": result.output,
                "metadata": result.metadata,
            });
            let semantic_error = automation_notion_tool_error(&parsed_output);
            invocation_parts.push(MessagePart::ToolInvocation {
                tool: tool.to_string(),
                args: args.clone(),
                result: Some(result_value.clone()),
                error: None,
            });
            call_rows.push(json!({
                "tool": tool,
                "args": args,
                "status": if semantic_error.is_some() { "failed" } else { "completed" },
                "result_excerpt": truncate_text(&result_value.to_string(), 1600),
                "error": semantic_error,
            }));
            if let Some(error) = semantic_error {
                Err(error)
            } else {
                Ok(parsed_output)
            }
        }
        Err(error) => {
            let error_text = error.to_string();
            invocation_parts.push(MessagePart::ToolInvocation {
                tool: tool.to_string(),
                args: args.clone(),
                result: None,
                error: Some(error_text.clone()),
            });
            call_rows.push(json!({
                "tool": tool,
                "args": args,
                "status": "failed",
                "error": error_text,
            }));
            Err(error_text)
        }
    }
}

pub(crate) async fn try_execute_notion_connector_writer_node(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    session_id: &str,
    workspace_root: &str,
    required_output_path: Option<&str>,
    requested_tools: &[String],
    upstream_inputs: &[Value],
) -> anyhow::Result<Option<Value>> {
    if !automation_node_is_notion_connector_writer(node) {
        return Ok(None);
    }
    let Some(output_path) = required_output_path else {
        return Ok(None);
    };
    let Some(data_source_id) = automation_notion_data_source_id(node) else {
        return Ok(None);
    };
    let Some(data_source_url) = automation_notion_data_source_url(node) else {
        return Ok(None);
    };
    let Some(source_node_id) = automation_notion_source_node_id(node) else {
        return Ok(None);
    };

    let resolved_output =
        resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    if let Some(parent) = resolved_output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let leads = automation_notion_writer_leads(node, upstream_inputs, &source_node_id);
    let status_value = automation_notion_status_property_value(node);
    let tenant_context = automation.tenant_context();
    let dispatch_scope = if requested_tools.is_empty() {
        vec![
            "mcp.notion.notion_fetch".to_string(),
            "mcp.notion.notion_search".to_string(),
            "mcp.notion.notion_create_pages".to_string(),
            "mcp.notion.notion_update_page".to_string(),
            "write".to_string(),
        ]
    } else {
        requested_tools.to_vec()
    };

    let mut invocation_parts = Vec::new();
    let mut call_rows = Vec::new();
    let mut errors = Vec::<String>::new();
    let mut inserted_thread_links = Vec::<String>::new();
    let mut updated_page_ids = Vec::<String>::new();
    let mut inserted_count = 0usize;
    let mut skipped_duplicate_count = 0usize;
    let mut failed_count = 0usize;

    if !leads.is_empty() {
        if let Err(error) = automation_dispatch_deterministic_tool(
            state,
            run_id,
            node,
            tenant_context.clone(),
            &dispatch_scope,
            "mcp.notion.notion_fetch",
            json!({ "id": data_source_url }),
            &mut invocation_parts,
            &mut call_rows,
        )
        .await
        {
            failed_count = leads.len();
            errors.push(format!("notion_fetch failed for {data_source_url}: {error}"));
        } else {
            for lead in &leads {
                let properties = automation_notion_lead_properties(lead, &status_value);
                let thread_link = properties
                    .get("Thread Link")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let user_handle = properties
                    .get("User Handle")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let title = properties
                    .get("Topic / Thread Title")
                    .and_then(Value::as_str)
                    .unwrap_or("Reddit lead")
                    .to_string();
                let mut duplicate_ref = None;
                for query in [&thread_link, &user_handle] {
                    if query.trim().is_empty() {
                        continue;
                    }
                    match automation_dispatch_deterministic_tool(
                        state,
                        run_id,
                        node,
                        tenant_context.clone(),
                        &dispatch_scope,
                        "mcp.notion.notion_search",
                        json!({ "query": query }),
                        &mut invocation_parts,
                        &mut call_rows,
                    )
                    .await
                    {
                        Ok(result) => {
                            if automation_notion_value_contains(&result, query) {
                                duplicate_ref = automation_notion_find_page_ref(&result);
                            }
                            if duplicate_ref.is_some() {
                                break;
                            }
                        }
                        Err(error) => errors.push(format!(
                            "notion_search failed for `{query}` from `{source_node_id}`: {error}"
                        )),
                    }
                }

                if let Some(page_ref) = duplicate_ref {
                    match automation_dispatch_deterministic_tool(
                        state,
                        run_id,
                        node,
                        tenant_context.clone(),
                        &dispatch_scope,
                        "mcp.notion.notion_update_page",
                        json!({
                            "command": "update_properties",
                            "page_id": page_ref,
                            "properties": properties,
                        }),
                        &mut invocation_parts,
                        &mut call_rows,
                    )
                    .await
                    {
                        Ok(_) => {
                            skipped_duplicate_count += 1;
                            updated_page_ids.push(page_ref);
                        }
                        Err(error) => {
                            failed_count += 1;
                            errors.push(format!("notion_update_page failed for duplicate `{thread_link}`: {error}"));
                        }
                    }
                    continue;
                }

                let create_args = json!({
                    "parent": {
                        "type": "data_source_id",
                        "data_source_id": data_source_id,
                    },
                    "pages": [
                        {
                            "parent": {
                                "type": "data_source_id",
                                "data_source_id": data_source_id,
                            },
                            "title": title,
                            "properties": properties,
                        }
                    ]
                });
                match automation_dispatch_deterministic_tool(
                    state,
                    run_id,
                    node,
                    tenant_context.clone(),
                    &dispatch_scope,
                    "mcp.notion.notion_create_pages",
                    create_args,
                    &mut invocation_parts,
                    &mut call_rows,
                )
                .await
                {
                    Ok(create_result) => {
                        let Some(created_ref) = automation_notion_find_page_ref(&create_result) else {
                            failed_count += 1;
                            errors.push(format!(
                                "notion_create_pages did not return a page id/url for `{thread_link}`"
                            ));
                            continue;
                        };
                        match automation_dispatch_deterministic_tool(
                            state,
                            run_id,
                            node,
                            tenant_context.clone(),
                            &dispatch_scope,
                            "mcp.notion.notion_update_page",
                            json!({
                                "command": "update_properties",
                                "page_id": created_ref,
                                "properties": automation_notion_lead_properties(lead, &status_value),
                            }),
                            &mut invocation_parts,
                            &mut call_rows,
                        )
                        .await
                        {
                            Ok(_) => {
                                inserted_count += 1;
                                if !thread_link.trim().is_empty() {
                                    inserted_thread_links.push(thread_link);
                                }
                                updated_page_ids.push(created_ref);
                            }
                            Err(error) => {
                                failed_count += 1;
                                errors.push(format!(
                                    "notion_update_page failed for created lead `{thread_link}`: {error}"
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        failed_count += 1;
                        errors.push(format!("notion_create_pages failed for `{thread_link}`: {error}"));
                    }
                }
            }
        }
    }

    let final_status = if errors.is_empty() { "completed" } else { "blocked" };
    let artifact = json!({
        "status": final_status,
        "inserted_count": inserted_count,
        "skipped_duplicate_count": skipped_duplicate_count,
        "failed_count": failed_count,
        "inserted_thread_links": inserted_thread_links,
        "updated_page_ids": updated_page_ids,
        "errors": errors,
        "notion_data_source_url": data_source_url,
        "source_node_id": source_node_id,
        "node_id": node.node_id,
        "run_id": run_id,
        "connector_evidence": call_rows,
    });
    let artifact_text = serde_json::to_string_pretty(&artifact)?;
    std::fs::write(&resolved_output, &artifact_text)?;

    let display_path = resolved_output
        .strip_prefix(workspace_root)
        .ok()
        .and_then(|value| value.to_str().map(str::to_string))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| output_path.to_string());
    invocation_parts.push(MessagePart::Text {
        text: format!(
            "Deterministic Notion connector writer `{}` wrote `{}`.\n\n{}",
            node.node_id, display_path, artifact_text
        ),
    });

    let mut session = state
        .storage
        .get_session(session_id)
        .await
        .unwrap_or_else(|| {
            Session::new(
                Some(format!(
                    "Automation {} / {}",
                    automation.automation_id, node.node_id
                )),
                Some(workspace_root.to_string()),
            )
        });
    session.project_id = Some(automation_workspace_project_id(workspace_root));
    session.workspace_root = Some(workspace_root.to_string());
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        invocation_parts,
    ));
    state.storage.save_session(session.clone()).await?;

    let artifact_validation = if final_status == "completed" {
        json!({
            "accepted_candidate_source": "deterministic_notion_connector_writer",
            "validation_outcome": "accepted",
            "unmet_requirements": [],
        })
    } else {
        json!({
            "accepted_candidate_source": "deterministic_notion_connector_writer",
            "validation_outcome": "blocked",
            "semantic_block_reason": artifact
                .get("errors")
                .cloned()
                .unwrap_or_else(|| json!([])),
            "unmet_requirements": ["notion_write_failed"],
        })
    };

    Ok(Some(
        node_output::wrap_automation_node_output_with_automation(
            automation,
            node,
            &session,
            requested_tools,
            session_id,
            Some(run_id),
            &format!("{{\"status\":\"{}\"}}", final_status),
            Some((display_path, artifact_text)),
            Some(artifact_validation),
        ),
    ))
}

