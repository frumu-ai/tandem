fn automation_node_text_suggests_notion_database_row_update(node_action_text: &str) -> bool {
    (node_action_text.contains("notion")
        || node_action_text.contains("database")
        || node_action_text.contains("row"))
        && (node_action_text.contains("update")
            || node_action_text.contains("save")
            || node_action_text.contains("write"))
        && (node_action_text.contains("database")
            || node_action_text.contains("row")
            || node_action_text.contains("table")
            || node_action_text.contains("result"))
}

fn automation_tool_name_is_notion_database_writer(tool: &str) -> bool {
    tool.trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .rsplit('.')
        .next()
        .is_some_and(|action| matches!(action, "notion_create_pages" | "notion_update_page"))
}

fn automation_tool_telemetry_mentions_notion_database_writer(tool_telemetry: &Value) -> bool {
    ["executed_tools", "requested_tools"].into_iter().any(|key| {
        tool_telemetry
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_str)
                    .any(automation_tool_name_is_notion_database_writer)
            })
    })
}

fn session_mentions_notion_database_writer_tool(session: &Session) -> bool {
    session.messages.iter().any(|message| {
        message.parts.iter().any(|part| {
            let MessagePart::ToolInvocation { tool, .. } = part else {
                return false;
            };
            automation_tool_name_is_notion_database_writer(tool)
        })
    })
}

fn metadata_string_value<'a>(metadata: &'a Value, key: &str) -> Option<&'a str> {
    metadata
        .get(key)
        .or_else(|| metadata.pointer(&format!("/builder/{key}")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn metadata_bool_value(metadata: &Value, key: &str) -> bool {
    metadata
        .get(key)
        .or_else(|| metadata.pointer(&format!("/builder/{key}")))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_has_notion_database_writer_metadata(node: &AutomationFlowNode) -> bool {
    if automation_node_is_notion_connector_writer(node) {
        return true;
    }

    let Some(metadata) = node.metadata.as_ref() else {
        return false;
    };

    let connector_is_notion = metadata_string_value(metadata, "connector")
        .is_some_and(|connector| connector.eq_ignore_ascii_case("notion"));
    let writer_kind_is_database_rows = metadata_string_value(metadata, "writer_kind").is_some_and(
        |writer_kind| {
            matches!(
                writer_kind.to_ascii_lowercase().replace('-', "_").as_str(),
                "database_rows" | "database_row" | "notion_database_rows"
            )
        },
    );
    let has_notion_target =
        metadata_string_value(metadata, "notion_data_source_id").is_some()
            || metadata_string_value(metadata, "notion_data_source_url").is_some()
            || metadata_string_value(metadata, "notion_database_id").is_some()
            || metadata_string_value(metadata, "notion_database_url").is_some();

    connector_is_notion
        && (metadata_bool_value(metadata, "connector_writer")
            || writer_kind_is_database_rows
            || has_notion_target)
}

fn automation_node_notion_database_row_update_requires_properties(
    node: &AutomationFlowNode,
    session: &Session,
    tool_telemetry: &Value,
    node_action_text: &str,
) -> bool {
    if !automation_node_text_suggests_notion_database_row_update(node_action_text) {
        return false;
    }

    automation_node_has_notion_database_writer_metadata(node)
        || automation_tool_telemetry_mentions_notion_database_writer(tool_telemetry)
        || session_mentions_notion_database_writer_tool(session)
}

fn session_has_notion_database_property_update(session: &Session) -> bool {
    const USER_VISIBLE_ROW_FIELDS: &[&str] = &[
        "Evidence",
        "Summary",
        "Sources",
        "Run ID",
        "Status",
        "Source",
        "Workflow",
        "Topic",
        "Completed At",
        "date:Completed At:start",
        "date:Completed At:is_datetime",
    ];

    session.messages.iter().any(|message| {
        message.parts.iter().any(|part| {
            let MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } = part
            else {
                return false;
            };
            if !tool
                .trim()
                .to_ascii_lowercase()
                .replace('-', "_")
                .rsplit('.')
                .next()
                .is_some_and(|action| action == "notion_update_page")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
                || automation_tool_result_failure_reason(result.as_ref()).is_some()
            {
                return false;
            }
            let Some(args) = tool_args_object(args) else {
                return false;
            };
            if args.get("command").and_then(Value::as_str) != Some("update_properties") {
                return false;
            }
            let Some(properties) = args.get("properties").and_then(Value::as_object) else {
                return false;
            };
            USER_VISIBLE_ROW_FIELDS
                .iter()
                .any(|field| properties.contains_key(*field))
        })
    })
}

fn automation_metadata_bool_value(metadata: &Value, key: &str) -> bool {
    metadata
        .get(key)
        .or_else(|| metadata.pointer(&format!("/builder/{key}")))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_metadata_string_value<'a>(metadata: &'a Value, key: &str) -> Option<&'a str> {
    metadata
        .get(key)
        .or_else(|| metadata.pointer(&format!("/builder/{key}")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn automation_node_expects_connector_source_materialization(node: &AutomationFlowNode) -> bool {
    let Some(metadata) = node.metadata.as_ref() else {
        return false;
    };
    if automation_metadata_bool_value(metadata, "connector_capture") {
        return true;
    }
    let connector_capture_enabled = metadata
        .get("connector_capture")
        .or_else(|| metadata.pointer("/builder/connector_capture"))
        .and_then(Value::as_object)
        .and_then(|object| object.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if connector_capture_enabled {
        return true;
    }
    automation_metadata_string_value(metadata, "artifact_type")
        .is_some_and(|artifact_type| artifact_type.to_ascii_lowercase().contains("source"))
}

fn automation_connector_capture_extracted_item_count(tool_telemetry: &Value) -> usize {
    tool_telemetry
        .pointer("/connector_capture/extracted_item_count")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn automation_source_array_key(key: &str) -> bool {
    matches!(
        key,
        "raw_posts"
            | "posts"
            | "raw_results"
            | "source_items"
            | "items"
            | "records"
            | "rows"
            | "threads"
            | "leads"
            | "results"
            | "documents"
            | "entries"
            | "hits"
            | "values"
    )
}

fn automation_array_has_materialized_source_rows(rows: &[Value]) -> bool {
    rows.iter().any(Value::is_object)
}

fn automation_artifact_json_has_materialized_source_rows(value: &Value, depth: usize) -> bool {
    if depth > 5 {
        return false;
    }
    match value {
        Value::Array(rows) => automation_array_has_materialized_source_rows(rows),
        Value::Object(object) => {
            for (key, child) in object {
                if automation_source_array_key(key)
                    && child
                        .as_array()
                        .is_some_and(|rows| automation_array_has_materialized_source_rows(rows))
                {
                    return true;
                }
            }
            object.values().any(|child| {
                automation_artifact_json_has_materialized_source_rows(child, depth + 1)
            })
        }
        _ => false,
    }
}

fn automation_connector_capture_source_rows_missing(
    node: &AutomationFlowNode,
    tool_telemetry: &Value,
    artifact_text: &str,
) -> bool {
    if !automation_node_expects_connector_source_materialization(node)
        || automation_connector_capture_extracted_item_count(tool_telemetry) == 0
    {
        return false;
    }
    let Ok(artifact) = serde_json::from_str::<Value>(artifact_text) else {
        return false;
    };
    !automation_artifact_json_has_materialized_source_rows(&artifact, 0)
}

fn automation_artifact_key_is_source_identity(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    normalized.contains("url")
        || normalized.contains("link")
        || normalized.contains("permalink")
        || normalized.contains("href")
        || normalized.contains("title")
}

fn automation_artifact_value_is_truncated_identity(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < 8 {
        return false;
    }
    trimmed.contains("...") || trimmed.contains('…')
}

fn automation_artifact_truncated_identity_value_path(value: &Value) -> Option<String> {
    fn visit(value: &Value, path: &mut Vec<String>, depth: usize) -> Option<String> {
        if depth > 16 {
            return None;
        }
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    path.push(key.clone());
                    if automation_artifact_key_is_source_identity(key) {
                        if let Some(text) = child.as_str() {
                            if automation_artifact_value_is_truncated_identity(text) {
                                return Some(path.join("."));
                            }
                        }
                    }
                    if let Some(found) = visit(child, path, depth + 1) {
                        return Some(found);
                    }
                    path.pop();
                }
                None
            }
            Value::Array(rows) => {
                for (index, child) in rows.iter().enumerate() {
                    path.push(index.to_string());
                    if let Some(found) = visit(child, path, depth + 1) {
                        return Some(found);
                    }
                    path.pop();
                }
                None
            }
            _ => None,
        }
    }

    visit(value, &mut Vec::new(), 0)
}

#[cfg(test)]
mod notion_database_writer_validation_tests {
    use super::*;

    fn test_node(node_id: &str, objective: &str, metadata: Value) -> AutomationFlowNode {
        AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: node_id.to_string(),
            agent_id: "agent".to_string(),
            objective: objective.to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            tool_policy: None,
            mcp_policy: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            metadata: Some(metadata),
        }
    }

    #[test]
    fn connector_source_node_does_not_require_notion_row_property_update() {
        let node = test_node(
            "search_agent_tool_security",
            "Search Reddit for how to secure agent tool calls, write the full source artifact, and pass results downstream for Notion insertion.",
            serde_json::json!({
                "artifact_type": "connector_source_research_shard",
                "connector_capture": {"enabled": true},
                "builder": {
                    "task_kind": "research",
                    "output_path": ".tandem/runs/run/artifacts/search-agent-tool-security.json"
                }
            }),
        );
        let session = Session::new(Some("source collection".to_string()), None);
        let telemetry = serde_json::json!({
            "requested_tools": [
                "mcp.composio_gmail.composio_multi_execute_tool",
                "write"
            ],
            "executed_tools": [
                "mcp.composio_gmail.composio_multi_execute_tool",
                "write"
            ],
            "failed_tools": []
        });
        let node_action_text = format!("{} {}", node.node_id, node.objective).to_ascii_lowercase();

        assert!(!automation_node_notion_database_row_update_requires_properties(
            &node,
            &session,
            &telemetry,
            &node_action_text,
        ));
    }

    #[test]
    fn notion_update_page_node_requires_user_visible_row_property_update() {
        let node = test_node(
            "update_notion_row",
            "Update the existing Notion database row with the completed report.",
            serde_json::json!({
                "builder": {
                    "output_path": "update-notion-row.md"
                }
            }),
        );
        let mut session = Session::new(Some("notion update".to_string()), None);
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "mcp.notion.notion_update_page".to_string(),
                args: serde_json::json!({
                    "command": "replace_content",
                    "page_id": "f3975ce71d8d45318bea2812c65f209b",
                    "new_str": "## Summary"
                }),
                result: Some(serde_json::json!({"page_id": "f3975ce7-1d8d-4531-8bea-2812c65f209b"})),
                error: None,
            }],
        ));
        let telemetry = serde_json::json!({
            "requested_tools": ["mcp.notion.notion_update_page", "write"],
            "executed_tools": ["mcp.notion.notion_update_page", "write"],
            "failed_tools": []
        });
        let node_action_text = format!("{} {}", node.node_id, node.objective).to_ascii_lowercase();

        assert!(automation_node_notion_database_row_update_requires_properties(
            &node,
            &session,
            &telemetry,
            &node_action_text,
        ));
    }

    #[test]
    fn connector_source_artifact_requires_materialized_rows_when_capture_found_items() {
        let node = test_node(
            "search_agent_tool_security",
            "Search Reddit for how to secure agent tool calls and write a source artifact.",
            serde_json::json!({
                "artifact_type": "connector_source_research_shard",
                "connector_capture": {"enabled": true}
            }),
        );
        let telemetry = serde_json::json!({
            "connector_capture": {
                "extracted_item_count": 2,
                "extracted_items_truncated": true
            }
        });

        assert!(automation_connector_capture_source_rows_missing(
            &node,
            &telemetry,
            r#"{"status":"completed","raw_posts":[]}"#
        ));
        assert!(!automation_connector_capture_source_rows_missing(
            &node,
            &telemetry,
            r#"{"status":"completed","raw_posts":[{"title":"Lead","url":"https://example.test"}]}"#
        ));
    }
}

pub(crate) fn automation_output_schema_validation_issue(
    schema: &Value,
    artifact: &Value,
) -> Option<String> {
    fn validate_node(schema: &Value, value: &Value, path: &str) -> Option<String> {
        if let Some(expected_const) = schema.get("const") {
            if value != expected_const {
                return Some(format!("{path} must equal {expected_const}"));
            }
        }

        if let Some(type_schema) = schema.get("type") {
            let type_matches = |expected: &str| match expected {
                "object" => value.is_object(),
                "array" => value.is_array(),
                "string" => value.is_string(),
                "number" => value.is_number(),
                "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
                "boolean" => value.is_boolean(),
                "null" => value.is_null(),
                _ => true,
            };
            let matches_type = type_schema
                .as_str()
                .map(type_matches)
                .or_else(|| {
                    type_schema
                        .as_array()
                        .map(|types| types.iter().filter_map(Value::as_str).any(type_matches))
                })
                .unwrap_or(true);
            if !matches_type {
                return Some(format!("{path} has the wrong JSON type"));
            }
        }

        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            let Some(object) = value.as_object() else {
                return Some(format!("{path} must be an object"));
            };
            for field in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    return Some(format!("{path}.{field} is required"));
                }
            }
        }

        if let (Some(properties), Some(object)) = (
            schema.get("properties").and_then(Value::as_object),
            value.as_object(),
        ) {
            for (field, child_schema) in properties {
                if let Some(child_value) = object.get(field) {
                    let child_path = if path == "$" {
                        format!("$.{field}")
                    } else {
                        format!("{path}.{field}")
                    };
                    if let Some(issue) = validate_node(child_schema, child_value, &child_path) {
                        return Some(issue);
                    }
                }
            }
        }

        if let (Some(item_schema), Some(items)) = (schema.get("items"), value.as_array()) {
            for (index, item) in items.iter().enumerate() {
                if let Some(issue) = validate_node(item_schema, item, &format!("{path}[{index}]")) {
                    return Some(issue);
                }
            }
        }

        None
    }

    validate_node(schema, artifact, "$")
}

pub(crate) fn validate_automation_artifact_output_with_context(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    run_id: Option<&str>,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
    upstream_evidence: Option<&AutomationUpstreamEvidence>,
    read_only_source_snapshot: Option<&std::collections::BTreeMap<String, Vec<u8>>>,
) -> (Option<(String, String)>, Value, Option<String>) {
    let suspicious_after = list_suspicious_automation_marker_files(workspace_root);
    let undeclared_files_created = suspicious_after
        .iter()
        .filter(|name| !workspace_snapshot_before.contains((*name).as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut auto_cleaned = false;
    if !suspicious_after.is_empty() {
        remove_suspicious_automation_marker_files(workspace_root);
        auto_cleaned = true;
    }

    let enforcement = automation_node_output_enforcement(node);
    let validator_kind = automation_output_validator_kind(node);
    let execution_policy = automation_node_execution_policy(node, workspace_root);
    let must_write_files =
        automation_node_must_write_files_for_automation(automation, node, runtime_values);
    let mutation_summary = session_file_mutation_summary(session, workspace_root);
    let verification_summary = session_verification_summary(node, session);
    let touched_files = mutation_summary
        .get("touched_files")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mutation_tool_by_file = mutation_summary
        .get("mutation_tool_by_file")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut rejected_reason = if undeclared_files_created.is_empty() {
        None
    } else {
        Some(format!(
            "undeclared marker files created: {}",
            undeclared_files_created.join(", ")
        ))
    };
    let mut semantic_block_reason = None::<String>;
    let mut unmet_requirements = Vec::<String>::new();
    let mut missing_required_concrete_mcp_tools = Vec::<String>::new();
    let mut read_only_source_mutations = Vec::<Value>::new();
    if let Some(snapshot) = read_only_source_snapshot {
        read_only_source_mutations = read_only_source_snapshot_mutations(workspace_root, snapshot);
        if !read_only_source_mutations.is_empty() {
            let _ = revert_read_only_source_snapshot_mutations(
                workspace_root,
                snapshot,
                &read_only_source_mutations,
            );
            let mutation_paths = read_only_source_mutations
                .iter()
                .filter_map(|value| value.get("path").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();
            unmet_requirements.push("read_only_source_mutations".to_string());
            if semantic_block_reason.is_none() {
                semantic_block_reason = Some(
                    "artifact blocked by attempted mutation of read-only source-of-truth input files"
                        .to_string(),
                );
            }
            if rejected_reason.is_none() {
                rejected_reason = Some(format!(
                    "read-only source-of-truth mutation detected: {}",
                    mutation_paths.join(", ")
                ));
            }
        }
    }
    let verified_output_materialized = verified_output.as_ref().is_some_and(|value| {
        tool_telemetry
            .get("verified_output_materialized_by_current_attempt")
            .and_then(Value::as_bool)
            .unwrap_or(true)
            && automation_verified_output_differs_from_preexisting(preexisting_output, value)
    });
    let verified_output_for_restore = verified_output.clone();
    let mut accepted_output = verified_output;
    let verified_output_nonterminal_status = accepted_output
        .as_ref()
        .and_then(|(_, text)| automation_artifact_json_status_is_nonterminal(text));
    let mut recovered_from_session_write = false;
    let quality_mode_resolution = enforcement::automation_node_quality_mode_resolution(node);
    let mut validation_basis = json!({
        "authority": "filesystem_and_receipts",
        "quality_mode": quality_mode_resolution.effective.stable_key(),
        "requested_quality_mode": quality_mode_resolution
            .requested
            .map(|mode| mode.stable_key()),
        "legacy_quality_rollback_enabled": quality_mode_resolution.legacy_rollback_enabled,
    });
    let current_read_paths = session_read_paths(session, workspace_root);
    let current_discovered_relevant_paths =
        session_discovered_relevant_paths(session, workspace_root);
    let use_upstream_evidence = automation_node_uses_upstream_validation_evidence(node);
    let upstream_read_paths = upstream_evidence
        .map(|evidence| evidence.read_paths.clone())
        .unwrap_or_default();
    let required_source_read_paths =
        enforcement::automation_node_required_source_read_paths_for_automation(
            automation,
            node,
            workspace_root,
            runtime_values,
        );
    let missing_required_source_read_paths = required_source_read_paths
        .iter()
        .filter(|path| {
            let current_read = current_read_paths.iter().any(|read| read == *path);
            let upstream_read =
                use_upstream_evidence && upstream_read_paths.iter().any(|read| read == *path);
            !current_read && !upstream_read
        })
        .cloned()
        .collect::<Vec<_>>();
    let required_connector_capture_read_paths =
        automation_required_connector_capture_read_paths(tool_telemetry);
    let missing_required_connector_capture_read_paths = required_connector_capture_read_paths
        .iter()
        .filter(|path| !current_read_paths.iter().any(|read| read == *path))
        .cloned()
        .collect::<Vec<_>>();
    if let Some(object) = validation_basis.as_object_mut() {
        object.insert(
            "required_source_read_paths".to_string(),
            json!(required_source_read_paths),
        );
        object.insert(
            "missing_required_source_read_paths".to_string(),
            json!(missing_required_source_read_paths),
        );
        object.insert(
            "required_connector_capture_read_paths".to_string(),
            json!(required_connector_capture_read_paths),
        );
        object.insert(
            "missing_required_connector_capture_read_paths".to_string(),
            json!(missing_required_connector_capture_read_paths),
        );
    }
    let explicit_input_files =
        automation_node_effective_input_files_for_automation(automation, node, runtime_values);
    let explicit_output_files =
        automation_node_effective_output_files_for_automation(automation, node, runtime_values);
    let mut read_paths = current_read_paths.clone();
    let mut discovered_relevant_paths = if use_upstream_evidence {
        let mut paths = Vec::new();
        if let Some(upstream) = upstream_evidence {
            read_paths.extend(upstream.read_paths.clone());
            paths.extend(upstream.discovered_relevant_paths.clone());
        }
        paths
    } else {
        current_discovered_relevant_paths.clone()
    };
    if !explicit_input_files.is_empty() {
        discovered_relevant_paths = explicit_input_files.clone();
    }
    read_paths.sort();
    read_paths.dedup();
    discovered_relevant_paths.sort();
    discovered_relevant_paths.dedup();
    let mut reviewed_paths_backed_by_read = Vec::<String>::new();
    let mut unreviewed_relevant_paths = Vec::<String>::new();
    let mut repair_attempted = false;
    let mut repair_succeeded = false;
    let mut citation_count = 0usize;
    let mut current_web_research_citations = Vec::<String>::new();
    let mut current_web_research_citation_count = 0usize;
    let mut web_sources_reviewed_present = false;
    let mut heading_count = 0usize;
    let mut paragraph_count = 0usize;
    let mut artifact_candidates = Vec::<Value>::new();
    let mut accepted_candidate_source = None::<String>;
    let mut blocked_handoff_cleanup_action = None::<String>;
    let mcp_grounded_citations_artifact =
        automation_node_is_mcp_grounded_citations_artifact(node, tool_telemetry);
    let execution_mode = execution_policy
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("artifact_write");
    let requires_current_attempt_output = execution_mode == "artifact_write"
        && automation_node_required_output_path(node).is_some()
        && !automation_node_allows_preexisting_output_reuse(node);
    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    let enforcement_requires_evidence = !enforcement.required_tools.is_empty()
        || !enforcement.required_evidence.is_empty()
        || !enforcement.required_sections.is_empty()
        || !enforcement.prewrite_gates.is_empty();
    let parsed_status = parse_status_json(session_text);
    let preserve_completed_generic_artifact = validator_kind
        == crate::AutomationOutputValidatorKind::GenericArtifact
        && parsed_status
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| value.eq_ignore_ascii_case("completed"));
    let mut structured_handoff =
        if validator_kind == crate::AutomationOutputValidatorKind::StructuredJson {
            extract_structured_handoff_json(session_text)
        } else {
            None
        };
    let repair_exhausted_hint = parsed_status
        .as_ref()
        .and_then(|value| value.get("repairExhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if rejected_reason.is_none() && matches!(execution_mode, "git_patch" | "filesystem_patch") {
        let unsafe_raw_write_paths = touched_files
            .iter()
            .filter(|path| workspace_snapshot_before.contains((*path).as_str()))
            .filter(|path| path_looks_like_source_file(path))
            .filter(|path| {
                mutation_tool_by_file
                    .get(*path)
                    .and_then(Value::as_array)
                    .is_some_and(|tools| {
                        let used_write = tools.iter().any(|value| value.as_str() == Some("write"));
                        let used_safe_patch = tools.iter().any(|value| {
                            matches!(value.as_str(), Some("edit") | Some("apply_patch"))
                        });
                        used_write && !used_safe_patch
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        if !unsafe_raw_write_paths.is_empty() {
            rejected_reason = Some(format!(
                "unsafe raw source rewrite rejected: {}",
                unsafe_raw_write_paths.join(", ")
            ));
        }
    }

    if let Some((path, text)) = accepted_output.clone() {
        let session_write_candidates = session_write_candidates_for_output(
            session,
            workspace_root,
            &path,
            run_id,
            runtime_values,
        );
        let requested_tools_for_contract = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let requested_concrete_mcp_tool = requested_tools_for_contract
            .iter()
            .any(|tool| tool.starts_with("mcp.") && tool != "mcp_list" && !tool.ends_with(".*"));
        let requested_has_read = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
        let requested_has_websearch = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| {
                tools
                    .iter()
                    .any(|value| value.as_str() == Some("websearch"))
            });
        let executed_has_mcp_list = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("mcp_list")));
        let current_executed_has_read = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
        let canonical_read_paths = automation_attempt_evidence_read_paths(tool_telemetry);
        let upstream_has_read = use_upstream_evidence
            && upstream_evidence.is_some_and(|evidence| !evidence.read_paths.is_empty());
        let executed_has_read =
            current_executed_has_read || !canonical_read_paths.is_empty() || upstream_has_read;
        let latest_web_research_failure = tool_telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str);
        let canonical_web_research_status =
            automation_attempt_evidence_web_research_status(tool_telemetry);
        let web_research_backend_unavailable = canonical_web_research_status
            .as_deref()
            .is_some_and(|status| status == "unavailable")
            || web_research_unavailable(latest_web_research_failure);
        let web_research_unavailable = !requested_has_websearch || web_research_backend_unavailable;
        let web_research_expected =
            enforcement_requires_external_sources(&enforcement) && !web_research_unavailable;
        let current_web_research_succeeded = canonical_web_research_status
            .as_deref()
            .is_some_and(|status| status == "succeeded")
            || tool_telemetry
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        let web_research_succeeded = current_web_research_succeeded
            || (use_upstream_evidence
                && upstream_evidence.is_some_and(|evidence| evidence.web_research_succeeded));
        current_web_research_citations = tool_telemetry
            .get("web_research_citations")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        current_web_research_citation_count = current_web_research_citations.len();
        let connector_discovery_text = automation_connector_hint_text(node);
        let explicit_node_tool_allowlist =
            super::node_runtime_impl::automation_node_metadata_tool_allowlist(node);
        let explicit_node_allows_no_mcp_tools =
            super::node_runtime_impl::automation_node_has_explicit_tool_policy(node)
                && !explicit_node_tool_allowlist
                    .iter()
                    .any(|tool| tool == "mcp_list" || tool.starts_with("mcp."));
        let connector_discovery_required =
            tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
                &connector_discovery_text,
            ) && !enforcement::automation_node_allows_optional_connector_references(node)
                && !explicit_node_allows_no_mcp_tools;
        let selected_mcp_server_names = tool_telemetry
            .get("capability_resolution")
            .and_then(|value| value.get("mcp_tool_diagnostics"))
            .and_then(|value| value.get("selected_servers"))
            .or_else(|| {
                tool_telemetry
                    .get("mcp_tool_diagnostics")
                    .and_then(|value| value.get("selected_servers"))
            })
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let connector_action_patterns =
            automation_requested_server_scoped_mcp_tools(node, &selected_mcp_server_names);
        let executed_concrete_mcp_tools = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|tool_name| {
                        *tool_name != "mcp_list"
                            && (connector_action_patterns.is_empty()
                                || connector_action_patterns.iter().any(|pattern| {
                                    tandem_core::tool_name_matches_policy(pattern, tool_name)
                                }))
                    })
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let executed_concrete_mcp_tool = !executed_concrete_mcp_tools.is_empty();
        let failed_tools_for_attempt = tool_telemetry
            .get("failed_tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let concrete_mcp_action_succeeded = executed_concrete_mcp_tools
            .iter()
            .any(|tool| !failed_tools_for_attempt.iter().any(|failed| failed == tool));
        let task_kind = automation_node_task_kind(node);
        let node_action_text = format!("{} {}", node.node_id, node.objective).to_ascii_lowercase();
        let notion_database_row_update_requires_properties =
            automation_node_notion_database_row_update_requires_properties(
                node,
                session,
                tool_telemetry,
                &node_action_text,
            );
        let notion_database_property_update_satisfied =
            !notion_database_row_update_requires_properties
                || session_has_notion_database_property_update(session);
        let connector_delivery_like_node = automation_node_is_outbound_action(node)
            || matches!(
                task_kind.as_deref(),
                Some(
                    "delivery"
                        | "connector_action"
                        | "external_action"
                        | "notion_update"
                        | "publish"
                )
            )
            || ((node_action_text.contains("notion")
                || node_action_text.contains("database")
                || node_action_text.contains("row"))
                && (node_action_text.contains("update")
                    || node_action_text.contains("save")
                    || node_action_text.contains("write")));
        let external_mutation_attempted = tool_telemetry
            .get("external_mutation_attempted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let external_mutation_succeeded = tool_telemetry
            .get("external_mutation_succeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let latest_external_mutation_failure = tool_telemetry
            .get("latest_external_mutation_failure")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("external mutation tool call failed");
        let connector_action_receipt_satisfied = concrete_mcp_action_succeeded
            && notion_database_property_update_satisfied
            && connector_delivery_like_node;
        if connector_delivery_like_node
            && external_mutation_attempted
            && !external_mutation_succeeded
        {
            unmet_requirements.push("external_mutation_failed".to_string());
            accepted_output = None;
            let reason = format!(
                "external delivery mutation failed and no later successful mutation was recorded: {}",
                latest_external_mutation_failure
            );
            if semantic_block_reason.is_none() {
                semantic_block_reason = Some(reason.clone());
            }
            if rejected_reason.is_none() {
                rejected_reason = Some(reason);
            }
        }
        if concrete_mcp_action_succeeded
            && notion_database_row_update_requires_properties
            && !notion_database_property_update_satisfied
        {
            unmet_requirements.push("notion_database_properties_not_updated".to_string());
            if semantic_block_reason.is_none() {
                semantic_block_reason = Some(
                    "Notion database row update did not update user-visible row properties"
                        .to_string(),
                );
            }
            if rejected_reason.is_none() {
                rejected_reason =
                    Some("notion database row properties were not updated".to_string());
            }
        }
        let workspace_inspection_satisfied = tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || executed_has_read
            || (use_upstream_evidence && !discovered_relevant_paths.is_empty());
        if connector_discovery_required
            && !executed_has_mcp_list
            && !executed_concrete_mcp_tool
            && !enforcement::automation_node_prefers_mcp_servers(node)
        {
            unmet_requirements.push("mcp_discovery_missing".to_string());
        }
        if automation_node_is_outbound_action(node)
            && !automation_node_requires_email_delivery(node)
            && !connector_action_patterns.is_empty()
            && !executed_concrete_mcp_tool
        {
            unmet_requirements.push("mcp_connector_action_missing".to_string());
        }
        if connector_discovery_required
            && !automation_node_is_outbound_action(node)
            && !connector_action_patterns.is_empty()
            && !executed_concrete_mcp_tool
        {
            unmet_requirements.push("mcp_connector_source_missing".to_string());
        }
        let mut required_concrete_mcp_tools = automation_node_required_concrete_mcp_tools(node);
        required_concrete_mcp_tools.extend(
            automation_node_required_tool_calls(node)
                .into_iter()
                .map(|call| call.tool)
                .filter(|tool| tool.starts_with("mcp.") && !tool.ends_with(".*")),
        );
        required_concrete_mcp_tools.sort();
        required_concrete_mcp_tools.dedup();
        let executed_tool_values = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        missing_required_concrete_mcp_tools = required_concrete_mcp_tools
            .iter()
            .filter(|required| {
                !executed_tool_values
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|tool_name| tandem_core::tool_name_matches_policy(required, tool_name))
            })
            .cloned()
            .collect();
        if !missing_required_concrete_mcp_tools.is_empty() {
            unmet_requirements.push("mcp_required_tool_missing".to_string());
        }
        let prewrite_requirements =
            automation_node_prewrite_requirements(node, &requested_tools_for_contract);
        let session_text_recovery_requires_prewrite =
            enforcement.session_text_recovery.as_deref() == Some("require_prewrite_satisfied");
        let session_text_recovery_allowed =
            prewrite_requirements.as_ref().is_none_or(|requirements| {
                !session_text_recovery_requires_prewrite
                    || repair_exhausted_hint
                    || ((!requirements.workspace_inspection_required
                        || workspace_inspection_satisfied)
                        && (!requirements.concrete_read_required || executed_has_read)
                        && (!requirements.successful_web_research_required
                            || web_research_succeeded))
            });
        let upstream_read_paths = upstream_evidence
            .map(|evidence| evidence.read_paths.clone())
            .unwrap_or_default();
        let upstream_citations = upstream_evidence
            .map(|evidence| evidence.citations.clone())
            .unwrap_or_default();
        let mut candidate_assessments = session_write_candidates
            .iter()
            .map(|candidate| {
                assess_artifact_candidate(
                    node,
                    workspace_root,
                    "session_write",
                    candidate,
                    &read_paths,
                    &discovered_relevant_paths,
                    &upstream_read_paths,
                    &upstream_citations,
                )
            })
            .collect::<Vec<_>>();
        let executed_tools_for_attempt = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let required_output_path =
            automation_node_required_output_path_with_runtime_for_run(node, run_id, runtime_values);
        let current_attempt_output_materialized_via_filesystem =
            required_output_path.as_ref().is_some_and(|output_path| {
                session_write_materialized_output_for_output(
                    session,
                    workspace_root,
                    output_path,
                    run_id,
                    runtime_values,
                )
            });
        let current_attempt_has_non_verified_activity = !executed_tools_for_attempt.is_empty()
            || !session_write_candidates.is_empty()
            || (use_upstream_evidence && upstream_evidence.is_some());
        let current_attempt_has_recorded_activity =
            current_attempt_has_non_verified_activity || verified_output_materialized;
        let connector_source_verified_output_has_rows =
            automation_node_expects_connector_source_materialization(node)
                && accepted_output.as_ref().is_some_and(|(_, text)| {
                    serde_json::from_str::<Value>(text)
                        .ok()
                        .is_some_and(|artifact| {
                            automation_artifact_json_has_materialized_source_rows(&artifact, 0)
                        })
                });
        let connector_source_output_is_run_scoped = run_id.is_some_and(|current_run_id| {
            required_output_path
                .as_deref()
                .is_some_and(|path| path.contains(current_run_id))
        });
        let connector_source_output_satisfied_by_capture =
            connector_source_verified_output_has_rows
                && connector_source_output_is_run_scoped
                && automation_connector_capture_extracted_item_count(tool_telemetry) > 0;
        let preexisting_output_reuse_allowed = automation_node_allows_preexisting_output_reuse(node)
            || connector_source_output_satisfied_by_capture;
        let current_attempt_output_materialized = current_attempt_output_materialized_via_filesystem
            || (verified_output_materialized && current_attempt_has_non_verified_activity)
            || connector_source_output_satisfied_by_capture;
        let must_write_file_statuses = must_write_files
            .iter()
            .map(|required_path| {
                let resolved = resolve_automation_output_path(workspace_root, required_path).ok();
                let exists = resolved
                    .as_ref()
                    .is_some_and(|path| path.exists() && path.is_file());
                let touched_by_current_attempt = session_write_touched_output_for_output(
                    session,
                    workspace_root,
                    required_path,
                    None,
                    runtime_values,
                );
                let materialized_by_current_attempt = session_write_materialized_output_for_output(
                    session,
                    workspace_root,
                    required_path,
                    None,
                    runtime_values,
                );
                json!({
                    "path": required_path,
                    "resolved_path": resolved.map(|path| path.to_string_lossy().to_string()),
                    "exists": exists,
                    "touched_by_current_attempt": touched_by_current_attempt,
                    "materialized_by_current_attempt": materialized_by_current_attempt,
                })
            })
            .collect::<Vec<_>>();
        validation_basis = json!({
            "authority": "filesystem_and_receipts",
            "quality_mode": quality_mode_resolution.effective.stable_key(),
            "requested_quality_mode": quality_mode_resolution
                .requested
                .map(|mode| mode.stable_key()),
            "legacy_quality_rollback_enabled": quality_mode_resolution.legacy_rollback_enabled,
            "current_attempt_output_materialized": current_attempt_output_materialized,
            "current_attempt_output_materialized_via_filesystem": current_attempt_output_materialized_via_filesystem,
            "verified_output_materialized": verified_output_materialized,
            "connector_source_output_satisfied_by_capture": connector_source_output_satisfied_by_capture,
            "required_output_path": required_output_path,
        });
        if let Some(object) = validation_basis.as_object_mut() {
            object.insert(
                "session_write_candidate_count".to_string(),
                json!(session_write_candidates.len()),
            );
            object.insert(
                "session_write_touched_output".to_string(),
                json!(session_write_touched_output_for_output(
                    session,
                    workspace_root,
                    &path,
                    run_id,
                    runtime_values,
                )),
            );
            object.insert(
                "current_attempt_has_recorded_activity".to_string(),
                json!(current_attempt_has_recorded_activity),
            );
            object.insert(
                "current_attempt_has_non_verified_activity".to_string(),
                json!(current_attempt_has_non_verified_activity),
            );
            object.insert(
                "current_attempt_has_read".to_string(),
                json!(current_executed_has_read || !canonical_read_paths.is_empty()),
            );
            object.insert(
                "current_attempt_has_web_research".to_string(),
                json!(current_web_research_succeeded),
            );
            object.insert(
                "workspace_inspection_satisfied".to_string(),
                json!(workspace_inspection_satisfied),
            );
            object.insert(
                "upstream_evidence_used".to_string(),
                json!(use_upstream_evidence && upstream_evidence.is_some()),
            );
            object.insert("must_write_files".to_string(), json!(must_write_files));
            object.insert(
                "explicit_input_files".to_string(),
                json!(explicit_input_files),
            );
            object.insert(
                "explicit_output_files".to_string(),
                json!(explicit_output_files),
            );
            object.insert(
                "must_write_file_statuses".to_string(),
                json!(must_write_file_statuses),
            );
        }
        if !must_write_files.is_empty()
            && !must_write_file_statuses.iter().all(|item| {
                item.get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
        {
            unmet_requirements.push("required_workspace_files_missing".to_string());
        }
        let missing_current_attempt_output_write = requires_current_attempt_output
            && !current_attempt_output_materialized
            && !preexisting_output_reuse_allowed;
        if !missing_current_attempt_output_write && !text.trim().is_empty() {
            candidate_assessments.push(assess_artifact_candidate(
                node,
                workspace_root,
                "verified_output",
                &text,
                &read_paths,
                &discovered_relevant_paths,
                &upstream_read_paths,
                &upstream_citations,
            ));
        }
        let allow_preexisting_candidate = if preexisting_output_reuse_allowed {
            true
        } else {
            !requires_current_attempt_output
                && !automation_node_is_strict_quality(node)
                && (!enforcement_requires_evidence || current_attempt_has_recorded_activity)
        };
        if allow_preexisting_candidate {
            if let Some(previous) = preexisting_output.filter(|value| !value.trim().is_empty()) {
                candidate_assessments.push(assess_artifact_candidate(
                    node,
                    workspace_root,
                    "preexisting_output",
                    previous,
                    &read_paths,
                    &discovered_relevant_paths,
                    &upstream_read_paths,
                    &upstream_citations,
                ));
            }
        }
        if missing_current_attempt_output_write {
            accepted_output = None;
            accepted_candidate_source = Some("current_attempt_missing_output_write".to_string());
            unmet_requirements.push("current_attempt_output_missing".to_string());
            let requested_read_missing = validator_kind
                == crate::AutomationOutputValidatorKind::ResearchBrief
                && tool_telemetry
                    .get("requested_tools")
                    .and_then(Value::as_array)
                    .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")))
                && !tool_telemetry
                    .get("executed_tools")
                    .and_then(Value::as_array)
                    .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
            rejected_reason = Some(if requested_read_missing {
                "research completed without concrete file reads or required source coverage"
                    .to_string()
            } else {
                format!(
                    "required output `{}` was not created in the current attempt",
                    path
                )
            });
        } else if !allow_preexisting_candidate {
            accepted_candidate_source = Some("current_attempt_missing_activity".to_string());
        }
        let best_candidate = best_artifact_candidate(&candidate_assessments);
        artifact_candidates = candidate_assessments
            .iter()
            .map(|candidate| {
                let accepted = best_candidate.as_ref().is_some_and(|best| {
                    best.source == candidate.source && best.text == candidate.text
                });
                artifact_candidate_summary(candidate, accepted)
            })
            .collect::<Vec<_>>();
        if let Some(best) = best_candidate.clone() {
            if !missing_current_attempt_output_write {
                accepted_candidate_source = Some(best.source.clone());
            }
            reviewed_paths_backed_by_read = best.reviewed_paths_backed_by_read.clone();
            citation_count = best.citation_count;
            web_sources_reviewed_present = best.web_sources_reviewed_present;
            heading_count = best.heading_count;
            paragraph_count = best.paragraph_count;
            if discovered_relevant_paths.is_empty() {
                discovered_relevant_paths = best.reviewed_paths.clone();
            }
            unreviewed_relevant_paths = best.unreviewed_relevant_paths.clone();
            let best_is_verified_output = best.source == "verified_output" && best.text == text;
            if !best_is_verified_output {
                if session_text_recovery_allowed {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, &path) {
                        let _ = std::fs::write(&resolved, &best.text);
                        accepted_output = Some((path.clone(), best.text.clone()));
                    }
                }
                recovered_from_session_write =
                    session_text_recovery_allowed && best.source == "session_write";
            } else {
                accepted_output = Some((path.clone(), best.text.clone()));
            }
        } else if missing_current_attempt_output_write {
            if rejected_reason.is_none() {
                rejected_reason = Some(format!(
                    "required output `{}` was not created in the current attempt",
                    path
                ));
            }
            semantic_block_reason =
                Some("required output was not created in the current attempt".to_string());
        }
        repair_attempted = session_write_candidates.len() > 1
            && (requested_has_read || web_research_expected)
            && (!reviewed_paths_backed_by_read.is_empty()
                || !read_paths.is_empty()
                || tool_telemetry
                    .get("tool_call_counts")
                    .and_then(|value| value.get("write"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 1);
        let selected_assessment = best_candidate.as_ref();
        let selected_text = selected_assessment
            .map(|assessment| assessment.text.as_str())
            .unwrap_or(text.as_str());
        let connector_source_unavailable_with_recorded_limitation = connector_discovery_required
            && !automation_node_is_outbound_action(node)
            && !connector_action_patterns.is_empty()
            && !requested_concrete_mcp_tool
            && !executed_concrete_mcp_tool
            && artifact_text_has_connector_source_evidence_or_limitation(selected_text);
        if connector_source_unavailable_with_recorded_limitation {
            unmet_requirements.retain(|item| item != "mcp_connector_source_missing");
        }
        if validator_kind == crate::AutomationOutputValidatorKind::StructuredJson
            && structured_handoff.is_none()
        {
            structured_handoff = extract_structured_handoff_json(selected_text);
        }
        if artifact_text_contains_required_tool_mode_failure(selected_text) {
            unmet_requirements.push("provider_required_tool_mode_unsatisfied".to_string());
            accepted_output = None;
            let reason = "artifact contains a provider required-tool/write-required failure marker"
                .to_string();
            if semantic_block_reason.is_none() {
                semantic_block_reason = Some(reason.clone());
            }
            if rejected_reason.is_none() {
                rejected_reason = Some(reason);
            }
        }
        let connector_inventory_only_artifact = connector_discovery_required
            && !automation_node_is_outbound_action(node)
            && !connector_action_patterns.is_empty()
            && artifact_text_is_mcp_inventory_only(selected_text);
        let connector_source_artifact_missing = connector_discovery_required
            && !automation_node_is_outbound_action(node)
            && !connector_action_patterns.is_empty()
            && (connector_inventory_only_artifact
                || (executed_concrete_mcp_tool
                    && !artifact_text_has_receipt_backed_connector_source_evidence(
                        selected_text,
                        selected_assessment,
                        &executed_concrete_mcp_tools,
                        &selected_mcp_server_names,
                    )));
        if connector_source_artifact_missing {
            unmet_requirements.push("mcp_connector_source_artifact_missing".to_string());
            accepted_output = None;
            let reason = "connector-backed source artifact contains connector inventory only; include source evidence from a concrete mcp.* tool result or an explicit connector limitation"
                .to_string();
            if semantic_block_reason.is_none() {
                semantic_block_reason = Some(reason.clone());
            }
            if rejected_reason.is_none() {
                rejected_reason = Some(reason);
            }
        }
        let required_tools_for_node = enforcement.required_tools.clone();
        let has_required_tools = !required_tools_for_node.is_empty();
        let failed_tools_for_node = tool_telemetry
            .get("failed_tools")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(|tool| tool.trim().to_ascii_lowercase().replace('-', "_"))
                    .filter(|tool| !tool.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let failed_required_tools = required_tools_for_node
            .iter()
            .map(|tool| tool.trim().to_ascii_lowercase().replace('-', "_"))
            .filter(|required| {
                failed_tools_for_node.iter().any(|failed| {
                    failed == required
                        || required
                            .strip_suffix(".*")
                            .is_some_and(|prefix| failed.starts_with(prefix))
                })
            })
            .collect::<Vec<_>>();
        if !failed_required_tools.is_empty() {
            unmet_requirements.push("mcp_required_tool_failed".to_string());
            accepted_output = None;
            let failure_detail = tool_telemetry
                .get("latest_tool_failure")
                .and_then(|value| value.get("reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("a required connector/tool call returned an error result");
            let reason = format!(
                "required tool call failed for {}: {}",
                failed_required_tools.join(", "),
                failure_detail
            );
            if semantic_block_reason.is_none() {
                semantic_block_reason = Some(reason.clone());
            }
            if rejected_reason.is_none() {
                rejected_reason = Some(reason);
            }
        }
        let validation_profile = enforcement
            .validation_profile
            .as_deref()
            .unwrap_or("artifact_only");
        let research_synthesis_contract = validation_profile == "research_synthesis";
        let requires_local_source_reads = enforcement
            .required_evidence
            .iter()
            .any(|item| item == "local_source_reads");
        let requires_external_sources = enforcement
            .required_evidence
            .iter()
            .any(|item| item == "external_sources")
            && !web_research_unavailable;
        let requires_files_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "files_reviewed");
        let requires_files_not_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "files_not_reviewed");
        let requires_citations = enforcement
            .required_sections
            .iter()
            .any(|item| item == "citations");
        let requires_web_sources_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "web_sources_reviewed")
            && !web_research_unavailable;
        let requires_local_source_reads = requires_local_source_reads
            && !mcp_grounded_citations_artifact
            && !research_synthesis_contract;
        let requires_external_sources =
            requires_external_sources && !mcp_grounded_citations_artifact;
        let requires_files_reviewed = requires_files_reviewed && !mcp_grounded_citations_artifact;
        let requires_files_not_reviewed =
            requires_files_not_reviewed && !mcp_grounded_citations_artifact;
        let requires_citations = requires_citations && !mcp_grounded_citations_artifact;
        let requires_web_sources_reviewed =
            requires_web_sources_reviewed && !mcp_grounded_citations_artifact;
        let has_research_contract = requires_local_source_reads
            || requires_external_sources
            || requires_files_reviewed
            || requires_files_not_reviewed
            || requires_citations
            || requires_web_sources_reviewed;
        let optional_workspace_reads =
            enforcement::automation_node_allows_optional_workspace_reads(node);
        let requires_read = required_tools_for_node.iter().any(|tool| tool == "read");
        let requires_websearch = required_tools_for_node
            .iter()
            .any(|tool| tool == "websearch")
            && !web_research_unavailable;
        if has_research_contract && (requested_has_read || requires_local_source_reads) {
            let missing_concrete_reads = !optional_workspace_reads
                && (requires_local_source_reads || requested_has_read)
                && !executed_has_read;
            let missing_named_source_reads = !missing_required_source_read_paths.is_empty();
            let files_reviewed_backed = selected_assessment.is_some_and(|assessment| {
                !assessment.reviewed_paths.is_empty()
                    && assessment.reviewed_paths.len()
                        == assessment.reviewed_paths_backed_by_read.len()
            });
            let missing_file_coverage = (requires_files_reviewed
                && !selected_assessment
                    .is_some_and(|assessment| assessment.files_reviewed_present))
                || (requires_files_reviewed && !files_reviewed_backed)
                || (requires_files_not_reviewed && !unreviewed_relevant_paths.is_empty());
            let missing_web_research = requires_external_sources && !web_research_succeeded;
            let upstream_has_citations =
                use_upstream_evidence && upstream_evidence.is_some_and(|e| e.citation_count > 0);
            let current_tool_has_citations = current_web_research_citation_count > 0;
            let missing_citations = requires_citations
                && !selected_assessment.is_some_and(|assessment| assessment.citation_count > 0)
                && !upstream_has_citations
                && !current_tool_has_citations;
            let upstream_web_sources_reviewed = use_upstream_evidence
                && upstream_evidence.is_some_and(|e| e.web_research_succeeded);
            let missing_web_sources_reviewed = requires_web_sources_reviewed
                && !selected_assessment
                    .is_some_and(|assessment| assessment.web_sources_reviewed_present)
                && !upstream_web_sources_reviewed;
            let preserve_current_attempt_output_missing = !current_attempt_output_materialized
                && unmet_requirements
                    .iter()
                    .any(|value| value == "current_attempt_output_missing");
            let had_read_only_source_mutation = unmet_requirements
                .iter()
                .any(|value| value == "read_only_source_mutations");
            unmet_requirements.clear();
            if had_read_only_source_mutation {
                unmet_requirements.push("read_only_source_mutations".to_string());
            }
            if preserve_current_attempt_output_missing {
                unmet_requirements.push("current_attempt_output_missing".to_string());
            }
            let path_hygiene_failure = selected_assessment.and_then(|assessment| {
                validate_path_array_hygiene(&assessment.reviewed_paths)
                    .or_else(|| validate_path_array_hygiene(&assessment.unreviewed_relevant_paths))
            });
            if path_hygiene_failure.is_some() {
                unmet_requirements.push("files_reviewed_contains_nonconcrete_paths".to_string());
            }
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_named_source_reads {
                unmet_requirements.push("required_source_paths_not_read".to_string());
            }
            if missing_citations {
                unmet_requirements.push("citations_missing".to_string());
            }
            if requires_files_reviewed
                && !selected_assessment.is_some_and(|assessment| assessment.files_reviewed_present)
            {
                unmet_requirements.push("files_reviewed_missing".to_string());
            }
            if requires_files_reviewed && !files_reviewed_backed {
                unmet_requirements.push("files_reviewed_not_backed_by_read".to_string());
            }
            let strict_unreviewed_check = use_upstream_evidence
                && upstream_evidence
                    .as_ref()
                    .is_some_and(|e| !e.discovered_relevant_paths.is_empty());
            if requires_files_not_reviewed
                && !unreviewed_relevant_paths.is_empty()
                && !strict_unreviewed_check
            {
                unmet_requirements.push("relevant_files_not_reviewed_or_skipped".to_string());
            }
            if missing_web_sources_reviewed {
                unmet_requirements.push("web_sources_reviewed_missing".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            let has_path_hygiene_failure = path_hygiene_failure.is_some();
            if missing_concrete_reads
                || missing_named_source_reads
                || missing_citations
                || missing_file_coverage
                || missing_web_sources_reviewed
                || missing_web_research
                || has_path_hygiene_failure
            {
                semantic_block_reason = Some(if has_path_hygiene_failure {
                    "research artifact contains non-concrete paths (wildcards or directory placeholders) in source audit"
                        .to_string()
                } else if missing_named_source_reads {
                    "research completed without reading the exact required source files".to_string()
                } else if missing_concrete_reads {
                    if automation_node_is_handoff_only_structured_json(node) {
                        "structured handoff completed without required concrete file reads"
                            .to_string()
                    } else {
                        "research completed without concrete file reads or required source coverage"
                            .to_string()
                    }
                } else if missing_file_coverage || !unreviewed_relevant_paths.is_empty() {
                    "research completed without covering or explicitly skipping relevant discovered files".to_string()
                } else if missing_web_research && requested_has_read && !current_executed_has_read {
                    "research completed without concrete file reads or required source coverage"
                        .to_string()
                } else if missing_web_research {
                    "research completed without required current web research".to_string()
                } else if !unreviewed_relevant_paths.is_empty() {
                    "research completed without covering or explicitly skipping relevant discovered files".to_string()
                } else if missing_citations {
                    "research completed without citation-backed claims".to_string()
                } else if missing_web_sources_reviewed {
                    "research completed without a web sources reviewed section".to_string()
                } else {
                    "research completed without a source-backed files reviewed section".to_string()
                });
            }
        }
        if !has_research_contract && has_required_tools {
            let missing_concrete_reads = !optional_workspace_reads
                && !research_synthesis_contract
                && requires_read
                && !executed_has_read;
            let missing_named_source_reads = !missing_required_source_read_paths.is_empty();
            let missing_web_research =
                requires_websearch && requires_external_sources && !web_research_succeeded;
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_named_source_reads {
                unmet_requirements.push("required_source_paths_not_read".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            if missing_concrete_reads || missing_named_source_reads || missing_web_research {
                semantic_block_reason = Some(if missing_named_source_reads {
                    "artifact finalized without reading the exact required source files".to_string()
                } else {
                    "artifact finalized without using required tools".to_string()
                });
            }
        }
        let web_research_artifact_contradicts_tool_receipts = web_research_succeeded
            && accepted_output.as_ref().is_some_and(|(_, artifact_text)| {
                artifact_text_contradicts_successful_web_research(artifact_text)
            });
        if web_research_artifact_contradicts_tool_receipts {
            unmet_requirements.push("web_research_artifact_contradicts_tool_receipts".to_string());
            semantic_block_reason = Some(
                "artifact claims web research was unavailable even though web research succeeded in this run"
                    .to_string(),
            );
            if rejected_reason.is_none() {
                rejected_reason = semantic_block_reason.clone();
            }
        }
        if research_synthesis_contract {
            if upstream_evidence.is_some_and(|evidence| evidence.notion_identity_unconfirmed)
                && synthesis_overstates_unconfirmed_notion_identity(selected_text)
            {
                unmet_requirements.push("upstream_notion_identity_overstated".to_string());
                semantic_block_reason = Some(
                    "synthesis overstated an upstream Notion inspection that was explicitly unconfirmed"
                        .to_string(),
                );
                if rejected_reason.is_none() {
                    rejected_reason = semantic_block_reason.clone();
                }
            }
            if upstream_evidence.is_some_and(|evidence| evidence.external_citations_missing)
                && synthesis_makes_uncited_market_claims(selected_text)
            {
                unmet_requirements
                    .push("uncited_market_claims_from_limited_web_artifact".to_string());
                semantic_block_reason = Some(
                    "synthesis made market/web-backed claims even though upstream external citations were missing"
                        .to_string(),
                );
                if rejected_reason.is_none() {
                    rejected_reason = semantic_block_reason.clone();
                }
            }
        }
        let strict_quality_mode = enforcement::automation_node_is_strict_quality(node);
        if strict_quality_mode
            && validator_kind == crate::AutomationOutputValidatorKind::GenericArtifact
        {
            let contract_kind = node
                .output_contract
                .as_ref()
                .map(|contract| contract.kind.trim().to_ascii_lowercase())
                .unwrap_or_default();
            let selected = selected_assessment.cloned();
            let upstream_citation_count = upstream_evidence
                .map(|evidence| evidence.citation_count)
                .unwrap_or(0);
            let upstream_read_count = upstream_evidence
                .map(|evidence| evidence.read_paths.len())
                .unwrap_or(0);
            let upstream_evidence_anchor_target =
                source_evidence_anchor_target(&upstream_read_paths, &upstream_citations);
            let upstream_web_research_succeeded = upstream_evidence
                .map(|evidence| evidence.web_research_succeeded)
                .unwrap_or(false);
            let requires_rich_upstream_synthesis =
                automation_node_uses_upstream_validation_evidence(node)
                    && matches!(contract_kind.as_str(), "report_markdown" | "text_summary");
            let requires_inline_source_sections = enforcement
                .required_sections
                .iter()
                .any(|section| matches!(section.as_str(), "citations" | "web_sources_reviewed"));
            let missing_editorial_substance = !connector_action_receipt_satisfied
                && matches!(contract_kind.as_str(), "report_markdown" | "text_summary")
                && !selected.as_ref().is_some_and(|assessment| {
                    !assessment.placeholder_like
                        && (assessment.substantive
                            || (assessment.length >= 120 && assessment.paragraph_count >= 1))
                });
            let missing_markdown_structure = !connector_action_receipt_satisfied
                && contract_kind == "report_markdown"
                && !selected.as_ref().is_some_and(|assessment| {
                    assessment.heading_count >= 1 && assessment.paragraph_count >= 2
                });
            let missing_upstream_synthesis = requires_rich_upstream_synthesis
                && (upstream_read_count > 0
                    || upstream_citation_count > 0
                    || upstream_web_research_succeeded)
                && !selected.as_ref().is_some_and(|assessment| {
                    !assessment.placeholder_like
                        && assessment.substantive
                        && assessment.length >= 600
                        && (assessment.heading_count >= 4
                            || (assessment.heading_count >= 2 && assessment.paragraph_count >= 2)
                            || (assessment.heading_count >= 2 && assessment.list_count >= 4))
                        && assessment.evidence_anchor_count >= upstream_evidence_anchor_target
                        && (!requires_inline_source_sections
                            || upstream_citation_count == 0
                            || assessment.citation_count >= 1
                            || assessment.web_sources_reviewed_present)
                });
            let bare_relative_artifact_href =
                matches!(contract_kind.as_str(), "report_markdown" | "text_summary")
                    && selected.as_ref().is_some_and(|assessment| {
                        contains_bare_tandem_artifact_href(&assessment.text)
                    });
            if missing_editorial_substance {
                unmet_requirements.push("editorial_substance_missing".to_string());
            }
            if contract_kind != "citations"
                && selected
                    .as_ref()
                    .is_some_and(|assessment| assessment.placeholder_like)
            {
                unmet_requirements.push("placeholder_artifact".to_string());
            }
            if missing_markdown_structure {
                unmet_requirements.push("markdown_structure_missing".to_string());
            }
            if missing_upstream_synthesis {
                unmet_requirements.push("upstream_evidence_not_synthesized".to_string());
            }
            if bare_relative_artifact_href {
                unmet_requirements.push("bare_relative_artifact_href".to_string());
            }
            if semantic_block_reason.is_none()
                && (missing_editorial_substance
                    || missing_markdown_structure
                    || missing_upstream_synthesis
                    || bare_relative_artifact_href)
            {
                semantic_block_reason = Some(if missing_upstream_synthesis {
                    "final artifact does not adequately synthesize the available upstream evidence"
                        .to_string()
                } else if missing_markdown_structure {
                    "editorial artifact is missing expected markdown structure".to_string()
                } else if bare_relative_artifact_href {
                    "final artifact contains a bare relative artifact href; use a canonical run-scoped link or plain text instead"
                        .to_string()
                } else {
                    "editorial artifact is too weak or placeholder-like".to_string()
                });
            }
        }
        let explicit_completed = parsed_status
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| value.eq_ignore_ascii_case("completed"));
        let writes_blocked_handoff_artifact = !explicit_completed
            && accepted_output
                .as_ref()
                .map(|(_, accepted_text)| accepted_text.to_ascii_lowercase())
                .is_some_and(|lowered| {
                    (lowered.contains("status: blocked")
                        || lowered.contains("blocked pending")
                        || lowered.contains("node produced a blocked handoff artifact"))
                        && (lowered.contains("cannot be finalized")
                            || lowered.contains("required source reads")
                            || lowered.contains("web research")
                            || lowered.contains("toolset available"))
                });
        if has_research_contract
            && semantic_block_reason.is_some()
            && writes_blocked_handoff_artifact
        {
            if let Some((path, _)) = accepted_output.as_ref() {
                if let Some(previous) = preexisting_output.filter(|value| !value.trim().is_empty())
                {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, path) {
                        let _ = std::fs::write(&resolved, previous);
                    }
                    accepted_output = None;
                    accepted_candidate_source = None;
                    blocked_handoff_cleanup_action =
                        Some("restored_preexisting_output".to_string());
                } else {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, path) {
                        let _ = std::fs::remove_file(&resolved);
                    }
                    accepted_output = None;
                    accepted_candidate_source = None;
                    blocked_handoff_cleanup_action = Some("removed_blocked_output".to_string());
                }
            }
        }
        let repair_promoted_after_write = repair_attempted
            && execution_mode == "artifact_write"
            && accepted_output.is_some()
            && session_write_touched_output_for_output(
                session,
                workspace_root,
                &path,
                run_id,
                runtime_values,
            );
        let repair_promoted_after_read_and_output_change = repair_attempted
            && execution_mode == "artifact_write"
            && accepted_output.is_some()
            && (current_executed_has_read || !canonical_read_paths.is_empty())
            && automation_repair_output_differs_from_preexisting(
                preexisting_output,
                accepted_output.as_ref(),
            );
        if !writes_blocked_handoff_artifact
            && (repair_promoted_after_write || repair_promoted_after_read_and_output_change)
        {
            semantic_block_reason = None;
            rejected_reason = None;
            let had_read_only_source_mutation = unmet_requirements
                .iter()
                .any(|value| value == "read_only_source_mutations");
            let had_web_research_artifact_contradiction = unmet_requirements
                .iter()
                .any(|value| value == "web_research_artifact_contradicts_tool_receipts");
            let had_external_mutation_failed = unmet_requirements
                .iter()
                .any(|value| value == "external_mutation_failed");
            unmet_requirements.clear();
            if had_read_only_source_mutation {
                unmet_requirements.push("read_only_source_mutations".to_string());
            }
            if had_external_mutation_failed {
                unmet_requirements.push("external_mutation_failed".to_string());
                semantic_block_reason = Some(
                    "external delivery mutation failed and no later successful mutation was recorded"
                        .to_string(),
                );
                rejected_reason = semantic_block_reason.clone();
            }
            if had_web_research_artifact_contradiction {
                unmet_requirements
                    .push("web_research_artifact_contradicts_tool_receipts".to_string());
                semantic_block_reason = Some(
                    "artifact claims web research was unavailable even though web research succeeded in this run"
                        .to_string(),
                );
                rejected_reason = semantic_block_reason.clone();
            } else if !had_external_mutation_failed {
                repair_succeeded = true;
            }
            if let Some(object) = validation_basis.as_object_mut() {
                object.insert(
                    "repair_promoted_after_write".to_string(),
                    json!(repair_promoted_after_write),
                );
                object.insert(
                    "repair_promoted_after_read_and_output_change".to_string(),
                    json!(repair_promoted_after_read_and_output_change),
                );
            }
        }
        if rejected_reason.is_none()
            && matches!(execution_mode, "git_patch" | "filesystem_patch")
            && preexisting_output.is_some()
            && path_looks_like_source_file(&path)
            && tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| {
                    tools.iter().any(|value| value.as_str() == Some("write"))
                        && !tools.iter().any(|value| value.as_str() == Some("edit"))
                        && !tools
                            .iter()
                            .any(|value| value.as_str() == Some("apply_patch"))
                })
        {
            rejected_reason =
                Some("code workflow used raw write without patch/edit safety".to_string());
        }
        if semantic_block_reason.is_some()
            && !recovered_from_session_write
            && selected_assessment.is_some_and(|assessment| !assessment.substantive)
        {
            // TODO(coding-hardening): Fold this recovery path into a single
            // artifact-finalization step that deterministically picks the best
            // candidate before node output is wrapped, instead of patching up the
            // final file after semantic validation fires.
            if let Some(best) = selected_assessment
                .filter(|assessment| assessment.substantive)
                .cloned()
            {
                if session_text_recovery_allowed {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, &path) {
                        let _ = std::fs::write(&resolved, &best.text);
                        accepted_output = Some((path.clone(), best.text.clone()));
                        recovered_from_session_write = best.source == "session_write";
                        repair_succeeded = true;
                        accepted_candidate_source = Some(best.source.clone());
                    }
                }
            }
        }
        if repair_attempted && semantic_block_reason.is_none() {
            repair_succeeded = true;
        }
        if semantic_block_reason.is_some()
            && enforcement_requires_evidence
            && !current_attempt_has_recorded_activity
            && !preserve_completed_generic_artifact
        {
            accepted_output = None;
        }
    }
    let nonterminal_artifact_status = accepted_output
        .as_ref()
        .and_then(|(_, text)| automation_artifact_json_status_is_nonterminal(text))
        .or_else(|| {
            if accepted_output.is_none() {
                verified_output_nonterminal_status.clone()
            } else {
                None
            }
        });
    if let Some(status) = nonterminal_artifact_status {
        accepted_output = None;
        unmet_requirements.push("artifact_status_not_terminal".to_string());
        if rejected_reason.is_none() {
            rejected_reason = Some(format!("artifact reported non-terminal status `{status}`"));
        }
        if semantic_block_reason.is_none() {
            semantic_block_reason =
                Some(format!("artifact reported non-terminal status `{status}`"));
        }
    }
    if accepted_output.is_some() && accepted_candidate_source.is_none() {
        accepted_candidate_source = Some("verified_output".to_string());
    }
    if let (Some((_, text)), Some(schema)) = (
        accepted_output.as_ref(),
        node.output_contract
            .as_ref()
            .and_then(|contract| contract.schema.as_ref()),
    ) {
        let schema_issue = serde_json::from_str::<Value>(text)
            .map_err(|err| format!("artifact is not valid JSON: {err}"))
            .and_then(|artifact| {
                automation_output_schema_validation_issue(schema, &artifact)
                    .map(Err)
                    .unwrap_or(Ok(()))
            })
            .err();
        if let Some(issue) = schema_issue {
            accepted_output = None;
            unmet_requirements.push("output_schema_invalid".to_string());
            let reason = format!("artifact does not match output_contract.schema: {issue}");
            rejected_reason = Some(reason.clone());
            semantic_block_reason = Some(reason);
        }
    }
    if let Some((_, text)) = accepted_output.as_ref() {
        if let Ok(artifact) = serde_json::from_str::<Value>(text) {
            if let Some(path) = automation_artifact_truncated_identity_value_path(&artifact) {
                accepted_output = None;
                unmet_requirements.push("truncated_source_identity_value".to_string());
                let reason = format!(
                    "artifact contains a truncated source identity value at `{path}`; read the full upstream artifact and write the complete title/link value"
                );
                if rejected_reason.is_none() {
                    rejected_reason = Some(reason.clone());
                }
                if semantic_block_reason.is_none() {
                    semantic_block_reason = Some(reason);
                }
            }
        }
    }
    if let Some((_, text)) = accepted_output.as_ref() {
        if automation_connector_capture_source_rows_missing(node, tool_telemetry, text) {
            accepted_output = None;
            unmet_requirements.push("connector_capture_items_not_materialized".to_string());
            let reason = "connector capture found source rows, but the output artifact did not materialize any source rows".to_string();
            if rejected_reason.is_none() {
                rejected_reason = Some(reason.clone());
            }
            if semantic_block_reason.is_none() {
                semantic_block_reason = Some(reason);
            }
        }
    }
    if validator_kind == crate::AutomationOutputValidatorKind::StructuredJson {
        let requested_tools = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let executed_tools = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let requested_has_concrete_mcp = requested_tools
            .iter()
            .filter_map(Value::as_str)
            .any(|tool| tool.starts_with("mcp.") && tool != "mcp_list" && !tool.ends_with(".*"));
        let executed_has_concrete_mcp = executed_tools
            .iter()
            .filter_map(Value::as_str)
            .any(|tool| tool.starts_with("mcp.") && tool != "mcp_list" && !tool.ends_with(".*"));
        let connector_source_satisfied = requested_has_concrete_mcp && executed_has_concrete_mcp;
        let requested_has_websearch = requested_tools
            .iter()
            .any(|value| value.as_str() == Some("websearch"));
        let executed_has_mcp_list = executed_tools
            .iter()
            .any(|value| value.as_str() == Some("mcp_list"));
        let executed_has_read = executed_tools
            .iter()
            .any(|value| value.as_str() == Some("read"));
        let latest_web_research_failure = tool_telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str);
        let web_research_unavailable =
            !requested_has_websearch || web_research_unavailable(latest_web_research_failure);
        let web_research_succeeded = tool_telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let workspace_inspection_satisfied = tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || executed_has_read
            || !current_discovered_relevant_paths.is_empty();
        let connector_discovery_text = [
            node.objective.as_str(),
            node.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("builder"))
                .and_then(Value::as_object)
                .and_then(|builder| builder.get("prompt"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ]
        .join("\n");
        let explicit_node_tool_allowlist =
            super::node_runtime_impl::automation_node_metadata_tool_allowlist(node);
        let explicit_node_allows_no_mcp_tools =
            super::node_runtime_impl::automation_node_has_explicit_tool_policy(node)
                && !explicit_node_tool_allowlist
                    .iter()
                    .any(|tool| tool == "mcp_list" || tool.starts_with("mcp."));
        let connector_discovery_required =
            tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
                &connector_discovery_text,
            ) && !enforcement::automation_node_allows_optional_connector_references(node)
                && !explicit_node_allows_no_mcp_tools;
        let validation_profile = enforcement
            .validation_profile
            .as_deref()
            .unwrap_or("artifact_only");
        let research_synthesis_contract = validation_profile == "research_synthesis";
        let requires_read = !research_synthesis_contract
            && enforcement.required_tools.iter().any(|tool| tool == "read");
        let requires_websearch = enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "websearch")
            && !web_research_unavailable;
        let requires_workspace_inspection = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "workspace_inspection");
        let requires_concrete_reads = !research_synthesis_contract
            && enforcement
                .prewrite_gates
                .iter()
                .any(|gate| gate == "concrete_reads");
        let requires_successful_web_research = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research")
            && !web_research_unavailable;
        let optional_workspace_reads =
            enforcement::automation_node_allows_optional_workspace_reads(node);

        if (automation_node_is_handoff_only_structured_json(node)
            || (validator_kind == crate::AutomationOutputValidatorKind::StructuredJson
                && accepted_output.is_none()))
            && structured_handoff.is_none()
        {
            unmet_requirements.push("structured_handoff_missing".to_string());
        }
        let required_workspace_writes_completed = validation_basis
            .get("must_write_file_statuses")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                !rows.is_empty()
                    && rows.iter().all(|item| {
                        item.get("materialized_by_current_attempt")
                            .and_then(Value::as_bool)
                            == Some(true)
                    })
            });
        if requires_workspace_inspection
            && !workspace_inspection_satisfied
            && !connector_source_satisfied
            && !required_workspace_writes_completed
        {
            unmet_requirements.push("workspace_inspection_required".to_string());
        }
        let missing_required_read = requires_read && !executed_has_read;
        let missing_concrete_reads =
            requires_concrete_reads && !executed_has_read && !connector_source_satisfied;
        if !optional_workspace_reads && (missing_required_read || missing_concrete_reads) {
            unmet_requirements.push("no_concrete_reads".to_string());
        }
        if !missing_required_source_read_paths.is_empty() {
            unmet_requirements.push("required_source_paths_not_read".to_string());
        }
        if !missing_required_connector_capture_read_paths.is_empty() {
            unmet_requirements.push("connector_capture_source_not_read".to_string());
        }
        if !optional_workspace_reads
            && requires_concrete_reads
            && !executed_has_read
            && !connector_source_satisfied
        {
            unmet_requirements.push("concrete_read_required".to_string());
        }
        if (requires_websearch || requires_successful_web_research) && !web_research_succeeded {
            unmet_requirements.push("missing_successful_web_research".to_string());
        }
        if connector_discovery_required
            && !executed_has_mcp_list
            && !executed_has_concrete_mcp
            && !enforcement::automation_node_prefers_mcp_servers(node)
        {
            unmet_requirements.push("mcp_discovery_missing".to_string());
        }
        let required_mcp_tools_for_contract = automation_node_required_tool_calls(node)
            .into_iter()
            .map(|call| call.tool)
            .filter(|tool| tool.starts_with("mcp.") && !tool.ends_with(".*"))
            .collect::<Vec<_>>();
        let missing_required_mcp_tools_for_contract = required_mcp_tools_for_contract
            .iter()
            .filter(|required| {
                !executed_tools
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|tool_name| tandem_core::tool_name_matches_policy(required, tool_name))
            })
            .cloned()
            .collect::<Vec<_>>();
        if !missing_required_mcp_tools_for_contract.is_empty() {
            missing_required_concrete_mcp_tools.extend(missing_required_mcp_tools_for_contract);
            missing_required_concrete_mcp_tools.sort();
            missing_required_concrete_mcp_tools.dedup();
            unmet_requirements.push("mcp_required_tool_missing".to_string());
        }
        unmet_requirements.sort();
        unmet_requirements.dedup();
    }
    let validation_profile = enforcement
        .validation_profile
        .clone()
        .unwrap_or_else(|| "artifact_only".to_string());
    if validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief
        && !unreviewed_relevant_paths.is_empty()
        && !repair_succeeded
        && !unmet_requirements
            .iter()
            .any(|value| value == "relevant_files_not_reviewed_or_skipped")
    {
        unmet_requirements.push("relevant_files_not_reviewed_or_skipped".to_string());
    }
    unmet_requirements.sort();
    unmet_requirements.dedup();
    let mut warning_requirements = unmet_requirements
        .iter()
        .filter(|item| validation_requirement_is_warning(&validation_profile, item))
        .cloned()
        .collect::<Vec<_>>();
    unmet_requirements.retain(|item| !validation_requirement_is_warning(&validation_profile, item));
    if validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief
        && quality_mode_resolution.effective == enforcement::AutomationQualityMode::StrictResearchV1
    {
        let promoted = warning_requirements
            .iter()
            .filter(|item| item.as_str() == "relevant_files_not_reviewed_or_skipped")
            .cloned()
            .collect::<Vec<_>>();
        if !promoted.is_empty() {
            unmet_requirements.extend(promoted);
            warning_requirements
                .retain(|item| item.as_str() != "relevant_files_not_reviewed_or_skipped");
        }
    }
    warning_requirements.sort();
    warning_requirements.dedup();
    if unmet_requirements.is_empty()
        && !warning_requirements.is_empty()
        && semantic_block_reason.as_deref().is_some_and(|reason| {
            matches!(
                reason,
                "editorial artifact is missing expected markdown structure"
                    | "editorial artifact is too weak or placeholder-like"
            )
        })
    {
        semantic_block_reason = None;
    }
    if let Some(reason) = semantic_block_reason_for_requirements(&unmet_requirements) {
        semantic_block_reason = Some(reason);
    }
    if unmet_requirements
        .iter()
        .any(|value| value == "relevant_files_not_reviewed_or_skipped")
        && rejected_reason.is_none()
    {
        rejected_reason = semantic_block_reason.clone();
    }
    if validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief
        && tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")))
        && !tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")))
        && semantic_block_reason.as_deref()
            == Some("research completed without required current web research")
        && !unmet_requirements
            .iter()
            .any(|value| value == "current_attempt_output_missing")
    {
        semantic_block_reason = Some(
            "research completed without concrete file reads or required source coverage"
                .to_string(),
        );
        rejected_reason = semantic_block_reason.clone();
    }
    if validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief
        && semantic_block_reason.as_deref()
            == Some("research completed without concrete file reads or required source coverage")
        && rejected_reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("required output `"))
        && !unmet_requirements
            .iter()
            .any(|value| value == "current_attempt_output_missing")
    {
        rejected_reason = semantic_block_reason.clone();
    }
    let preserve_completed_generic_artifact = validator_kind
        == crate::AutomationOutputValidatorKind::GenericArtifact
        && parsed_status
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| value.eq_ignore_ascii_case("completed"));
    if unmet_requirements.iter().any(|requirement| {
        matches!(
            requirement.as_str(),
            "read_only_source_mutations" | "provider_required_tool_mode_unsatisfied"
        )
    }) {
        rejected_reason = semantic_block_reason.clone();
    }
    if unmet_requirements
        .iter()
        .any(|requirement| requirement == "mcp_required_tool_missing")
        && !missing_required_concrete_mcp_tools.is_empty()
    {
        let missing_tools = missing_required_concrete_mcp_tools.join(", ");
        semantic_block_reason = Some(format!(
            "required MCP tool calls were not completed in this attempt: {missing_tools}"
        ));
    }
    let missing_required_workspace_files = validation_basis
        .get("must_write_file_statuses")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|item| {
                    item.get("materialized_by_current_attempt")
                        .and_then(Value::as_bool)
                        != Some(true)
                })
                .filter_map(|item| item.get("path").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if unmet_requirements
        .iter()
        .any(|value| value == "required_workspace_files_missing")
        && !missing_required_workspace_files.is_empty()
    {
        let reason = if validator_kind == crate::AutomationOutputValidatorKind::StructuredJson
            && !automation_node_explicit_output_files(node).is_empty()
        {
            "required workspace files were not written for this run".to_string()
        } else {
            format!(
                "required workspace files were not written in the current attempt: {}",
                missing_required_workspace_files.join(", ")
            )
        };
        semantic_block_reason = Some(reason.clone());
        if rejected_reason.is_none() {
            rejected_reason = Some(reason);
        }
    }
    if unmet_requirements.iter().any(|requirement| {
        matches!(
            requirement.as_str(),
            "placeholder_artifact"
                | "provider_required_tool_mode_unsatisfied"
                | "mcp_required_tool_missing"
                | "mcp_connector_source_missing"
                | "mcp_connector_source_artifact_missing"
                | "mcp_discovery_missing"
                | "required_source_paths_not_read"
                | "web_research_artifact_contradicts_tool_receipts"
        ) || (!preserve_completed_generic_artifact
            && validator_kind != crate::AutomationOutputValidatorKind::ResearchBrief
            && matches!(
                requirement.as_str(),
                "no_concrete_reads" | "concrete_read_required" | "missing_successful_web_research"
            ))
    }) {
        accepted_output = None;
    }
    if should_downgrade_auto_cleaned_marker_rejection(
        rejected_reason.as_deref(),
        auto_cleaned,
        semantic_block_reason.as_deref(),
        accepted_output.is_some(),
    ) {
        rejected_reason = None;
        warning_requirements.push("undeclared_marker_files_cleaned".to_string());
        warning_requirements.sort();
        warning_requirements.dedup();
    }
    let required_output_path_for_retry =
        automation_node_required_output_path_with_runtime_for_run(node, run_id, runtime_values);
    let current_attempt_output_materialized_for_retry = required_output_path_for_retry
        .as_ref()
        .is_some_and(|output_path| {
            session_write_materialized_output_for_output(
                session,
                workspace_root,
                output_path,
                run_id,
                runtime_values,
            ) || (verified_output_materialized
                && validation_basis
                    .get("current_attempt_has_non_verified_activity")
                    .and_then(Value::as_bool)
                    .unwrap_or(false))
        });
    if accepted_output.is_none()
        && requires_current_attempt_output
        && !current_attempt_output_materialized_for_retry
        && !automation_node_allows_preexisting_output_reuse(node)
    {
        if rejected_reason.is_none() {
            let missing_output_path = required_output_path_for_retry
                .clone()
                .unwrap_or_else(|| automation_node_required_output_path(node).unwrap_or_default());
            rejected_reason = Some(format!(
                "required output `{}` was not created in the current attempt",
                missing_output_path
            ));
        }
        if !unmet_requirements
            .iter()
            .any(|value| value == "current_attempt_output_missing")
        {
            unmet_requirements.push("current_attempt_output_missing".to_string());
        }
        if use_upstream_evidence
            && upstream_evidence.is_some_and(|evidence| {
                !evidence.read_paths.is_empty() || evidence.citation_count > 0
            })
            && !unmet_requirements
                .iter()
                .any(|value| value == "upstream_evidence_not_synthesized")
        {
            unmet_requirements.push("upstream_evidence_not_synthesized".to_string());
        }
        if semantic_block_reason.is_none() {
            semantic_block_reason =
                Some("required output was not created in the current attempt".to_string());
        }
        if preexisting_output.is_some_and(|previous| previous != session_text) {
            semantic_block_reason =
                Some("required output was not created in the current attempt".to_string());
            let missing_output_path = required_output_path_for_retry
                .clone()
                .unwrap_or_else(|| automation_node_required_output_path(node).unwrap_or_default());
            rejected_reason = Some(format!(
                "required output `{}` was not created in the current attempt",
                missing_output_path
            ));
        }
    }
    if validator_kind == crate::AutomationOutputValidatorKind::StructuredJson
        && unmet_requirements
            .iter()
            .any(|value| value == "structured_handoff_missing")
    {
        semantic_block_reason =
            Some("structured handoff was not returned in the final response".to_string());
        rejected_reason = semantic_block_reason.clone();
    }
    let fintech_compliance_brief_validation =
        if automation_node_requires_fintech_compliance_brief_validation(automation, node) {
            let connector_proof = session_fintech_connector_proof(session);
            let report = accepted_output
                .as_ref()
                .map(|(_, text)| {
                    serde_json::from_str::<Value>(text)
                        .map(|artifact| {
                            tandem_core::validate_fintech_compliance_brief_artifact(
                                &artifact,
                                &connector_proof,
                            )
                        })
                        .unwrap_or_else(|_| tandem_core::FintechArtifactValidationReport {
                            passed: false,
                            issues: vec!["artifact_json_invalid".to_string()],
                        })
                })
                .unwrap_or_else(|| tandem_core::FintechArtifactValidationReport {
                    passed: false,
                    issues: vec!["artifact_missing".to_string()],
                });
            if let Some(object) = validation_basis.as_object_mut() {
                object.insert(
                    "fintech_connector_proof".to_string(),
                    json!(connector_proof),
                );
                object.insert(
                    "fintech_compliance_brief_validation".to_string(),
                    serde_json::to_value(&report).unwrap_or_else(|_| Value::Null),
                );
            }
            if !report.passed {
                accepted_output = None;
                unmet_requirements.push("fintech_compliance_brief_invalid".to_string());
                for issue in &report.issues {
                    unmet_requirements.push(format!("fintech_{issue}"));
                }
                unmet_requirements.sort();
                unmet_requirements.dedup();
                let reason = format!(
                    "fintech compliance brief failed validation: {}",
                    report.issues.join(", ")
                );
                semantic_block_reason = Some(reason.clone());
                if rejected_reason.is_none() {
                    rejected_reason = Some(reason);
                }
            }
            Some(report)
        } else {
            None
        };
    let active_profile = automation
        .execution
        .profile
        .unwrap_or(crate::automation_v2::execution_profile::ExecutionProfile::Strict);
    let scaled_repair_budget = enforcement.repair_budget.map(|budget| {
        crate::automation_v2::execution_profile::effective_repair_budget(budget, active_profile)
    });
    let (repair_attempt, repair_attempts_remaining, mut repair_exhausted) =
        infer_artifact_repair_state(
            parsed_status.as_ref(),
            repair_attempted,
            repair_succeeded,
            semantic_block_reason.as_deref(),
            tool_telemetry,
            scaled_repair_budget,
        );
    let truncated_source_identity_value = unmet_requirements
        .iter()
        .any(|value| value == "truncated_source_identity_value");
    let effective_node_max_attempts = tool_telemetry_u32(tool_telemetry, "node_max_attempts")
        .map(|max_attempts| {
            if truncated_source_identity_value {
                max_attempts.max(3)
            } else {
                max_attempts
            }
        });
    let node_attempt_has_retry_remaining = tool_telemetry_u32(tool_telemetry, "node_attempt")
        .zip(effective_node_max_attempts)
        .is_some_and(|(attempt, max_attempts)| attempt < max_attempts);
    let node_attempt_exhausted = tool_telemetry_u32(tool_telemetry, "node_attempt")
        .zip(effective_node_max_attempts)
        .is_some_and(|(attempt, max_attempts)| attempt >= max_attempts);
    let external_mutation_failed = unmet_requirements
        .iter()
        .any(|value| value == "external_mutation_failed");
    let repairable_contract_miss_with_node_budget = node_attempt_has_retry_remaining
        && unmet_requirements.iter().any(|value| {
            matches!(
                value.as_str(),
                "mcp_connector_source_missing"
                    | "mcp_connector_source_artifact_missing"
                    | "mcp_required_tool_missing"
                    | "external_mutation_failed"
                    | "structured_handoff_missing"
                    | "truncated_source_identity_value"
            ) || (value == "missing_successful_web_research"
                && validator_kind != crate::AutomationOutputValidatorKind::ResearchBrief)
                || (value == "no_concrete_reads"
                    && (automation_node_is_handoff_only_structured_json(node)
                        || validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief))
                || (value == "concrete_read_required"
                    && (automation_node_is_handoff_only_structured_json(node)
                        || validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief))
        });
    if !repair_exhausted_hint
        && (repairable_contract_miss_with_node_budget
            || (!node_attempt_exhausted
                && unmet_requirements.iter().any(|value| {
                    value == "structured_handoff_missing"
                        || (value == "no_concrete_reads"
                            && (automation_node_is_handoff_only_structured_json(node)
                                || validator_kind
                                    == crate::AutomationOutputValidatorKind::ResearchBrief))
                        || (value == "concrete_read_required"
                            && (automation_node_is_handoff_only_structured_json(node)
                                || validator_kind
                                    == crate::AutomationOutputValidatorKind::ResearchBrief))
                })))
    {
        repair_exhausted = false;
    }
    let has_required_tools = !enforcement.required_tools.is_empty();
    let schema_validation_failed = unmet_requirements
        .iter()
        .any(|value| value == "output_schema_invalid");
    let contract_requires_repair = validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || !enforcement.retry_on_missing.is_empty()
        || has_required_tools
        || external_mutation_failed
        || validator_kind == crate::AutomationOutputValidatorKind::StructuredJson
        || schema_validation_failed
        || truncated_source_identity_value;
    let current_attempt_has_recorded_activity = validation_basis
        .get("current_attempt_has_recorded_activity")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let structured_handoff_missing = unmet_requirements
        .iter()
        .any(|value| value == "structured_handoff_missing");
    if automation_node_is_handoff_only_structured_json(node)
        && !structured_handoff_missing
        && unmet_requirements
            .iter()
            .any(|value| value == "no_concrete_reads")
    {
        let reason =
            "structured handoff completed without required concrete file reads".to_string();
        semantic_block_reason = Some(reason.clone());
        rejected_reason = Some(reason);
    }
    let hard_blocking_unmet_requirements = unmet_requirements.iter().any(|value| {
        matches!(
            value.as_str(),
            "read_only_source_mutations"
                | "artifact_status_not_terminal"
                | "required_workspace_files_missing"
        )
    }) || (!current_attempt_has_recorded_activity
        && unmet_requirements.iter().any(|value| {
            matches!(
                value.as_str(),
                "provider_required_tool_mode_unsatisfied"
                    | "mcp_required_tool_missing"
                    | "mcp_connector_source_missing"
                    | "mcp_connector_source_artifact_missing"
                    | "mcp_discovery_missing"
                    | "required_source_paths_not_read"
            ) || (value == "no_concrete_reads"
                && !preserve_completed_generic_artifact
                && !automation_node_is_handoff_only_structured_json(node)
                && validator_kind != crate::AutomationOutputValidatorKind::ResearchBrief)
                || (value == "concrete_read_required"
                    && !preserve_completed_generic_artifact
                    && !automation_node_is_handoff_only_structured_json(node)
                    && validator_kind != crate::AutomationOutputValidatorKind::ResearchBrief)
        })
        && !structured_handoff_missing);
    if hard_blocking_unmet_requirements {
        accepted_output = None;
    }
    if unmet_requirements
        .iter()
        .any(|value| value == "external_mutation_failed")
    {
        accepted_output = None;
    }
    let validation_outcome = if unmet_requirements
        .iter()
        .any(|value| value == "current_attempt_output_missing")
        && preexisting_output.is_some()
    {
        "blocked"
    } else if unmet_requirements
        .iter()
        .any(|value| value == "current_attempt_output_missing")
        && !node_attempt_exhausted
    {
        "needs_repair"
    } else if unmet_requirements.iter().any(|value| {
        matches!(
            value.as_str(),
            "upstream_notion_identity_overstated"
                | "uncited_market_claims_from_limited_web_artifact"
        )
    }) {
        "blocked"
    } else if validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief
        && unmet_requirements
            .iter()
            .any(|value| value == "no_concrete_reads" || value == "concrete_read_required")
        || semantic_block_reason.as_deref()
            == Some("research completed without concrete file reads or required source coverage")
    {
        "needs_repair"
    } else if hard_blocking_unmet_requirements {
        "blocked"
    } else if contract_requires_repair && semantic_block_reason.is_some() {
        if repair_exhausted || hard_blocking_unmet_requirements {
            "blocked"
        } else {
            "needs_repair"
        }
    } else if semantic_block_reason.is_some() {
        "blocked"
    } else if !warning_requirements.is_empty() {
        "accepted_with_warnings"
    } else {
        "passed"
    };
    if external_mutation_failed {
        accepted_output = None;
        accepted_candidate_source = None;
    }
    if preserve_completed_generic_artifact
        && accepted_output.is_none()
        && !external_mutation_failed
        && !unmet_requirements
            .iter()
            .any(|value| value == "current_attempt_output_missing")
        && !unmet_requirements
            .iter()
            .any(|value| value == "output_schema_invalid")
    {
        if let Some((path, text)) = verified_output_for_restore {
            accepted_output = Some((path.clone(), text.clone()));
            if accepted_candidate_source.is_none() {
                accepted_candidate_source = Some("verified_output".to_string());
            }
        }
    }
    let should_classify = contract_requires_repair;
    let latest_web_research_failure = tool_telemetry
        .get("latest_web_research_failure")
        .and_then(Value::as_str);
    let requested_has_websearch = tool_telemetry
        .get("requested_tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools
                .iter()
                .any(|value| value.as_str() == Some("websearch"))
        });
    let web_research_expected_for_classification =
        enforcement_requires_external_sources(&enforcement)
            && requested_has_websearch
            && !web_research_unavailable(latest_web_research_failure);
    let external_research_mode = if enforcement_requires_external_sources(&enforcement) {
        if !requested_has_websearch || web_research_unavailable(latest_web_research_failure) {
            "waived_unavailable"
        } else {
            "required"
        }
    } else {
        "not_required"
    };
    let blocking_classification = if should_classify {
        classify_research_validation_state(
            &tool_telemetry
                .get("requested_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            &tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            web_research_expected_for_classification,
            &unmet_requirements,
            latest_web_research_failure,
            repair_exhausted,
        )
        .map(str::to_string)
    } else {
        None
    };
    let mut required_next_tool_actions = if should_classify {
        research_required_next_tool_actions(
            &tool_telemetry
                .get("requested_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            &tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            web_research_expected_for_classification,
            &unmet_requirements,
            &missing_required_source_read_paths,
            &upstream_evidence
                .map(|e| e.read_paths.clone())
                .unwrap_or_default(),
            &upstream_evidence
                .map(|e| e.citations.clone())
                .unwrap_or_default(),
            &unreviewed_relevant_paths,
            latest_web_research_failure,
        )
    } else {
        Vec::new()
    };
    if !missing_required_workspace_files.is_empty() {
        let missing_targets = missing_required_workspace_files
            .iter()
            .map(|path| format!("`{}`", path))
            .collect::<Vec<_>>()
            .join(", ");
        required_next_tool_actions.push(format!(
            "Write the required workspace file(s) {missing_targets} in this attempt before writing the run artifact; do not rely on the run artifact to satisfy this workspace-write contract."
        ));
        required_next_tool_actions.sort();
        required_next_tool_actions.dedup();
    }
    if !missing_required_concrete_mcp_tools.is_empty() {
        let missing_tools = missing_required_concrete_mcp_tools
            .iter()
            .map(|tool| format!("`{tool}`"))
            .collect::<Vec<_>>()
            .join(", ");
        required_next_tool_actions.push(format!(
            "Call the required MCP tool(s) {missing_tools} in this same attempt before writing the run artifact."
        ));
        required_next_tool_actions.sort();
        required_next_tool_actions.dedup();
    }

    if unmet_requirements
        .iter()
        .any(|value| value == "required_source_paths_not_read")
    {
        let reason =
            "research completed without reading the exact required source files".to_string();
        rejected_reason = Some(reason.clone());
        semantic_block_reason = Some(reason);
    }

    let metadata = json!({
        "accepted_artifact_path": accepted_output.as_ref().map(|(path, _)| path.clone()),
        "accepted_candidate_source": accepted_candidate_source,
        "rejected_artifact_reason": rejected_reason,
        "semantic_block_reason": semantic_block_reason,
        "recovered_from_session_write": recovered_from_session_write,
        "undeclared_files_created": undeclared_files_created,
        "auto_cleaned": auto_cleaned,
        "execution_policy": execution_policy,
        "touched_files": touched_files,
        "mutation_tool_by_file": Value::Object(mutation_tool_by_file),
        "read_only_source_mutation_events": Value::Array(read_only_source_mutations.clone()),
        "read_only_source_mutation_count": read_only_source_mutations.len(),
        "verification": verification_summary,
        "git_diff_summary": git_diff_summary_for_paths(workspace_root, &touched_files),
        "read_paths": read_paths,
        "upstream_read_paths": if use_upstream_evidence {
            json!(upstream_evidence.map_or(&[] as &[_], |e| e.read_paths.as_slice()))
        } else {
            json!([])
        },
        "current_node_read_paths": current_read_paths,
        "discovered_relevant_paths": discovered_relevant_paths,
        "current_node_discovered_relevant_paths": current_discovered_relevant_paths,
        "reviewed_paths_backed_by_read": reviewed_paths_backed_by_read,
        "unreviewed_relevant_paths": unreviewed_relevant_paths,
        "citation_count": if use_upstream_evidence {
            json!(citation_count.saturating_add(
                upstream_evidence.map(|e| e.citation_count).unwrap_or(0)
            ).saturating_add(current_web_research_citation_count))
        } else {
            json!(citation_count.saturating_add(current_web_research_citation_count))
        },
        "current_web_research_citations": current_web_research_citations,
        "current_web_research_citation_count": current_web_research_citation_count,
        "upstream_citations": if use_upstream_evidence {
            json!(upstream_evidence.map_or(&[] as &[_], |e| e.citations.as_slice()))
        } else {
            json!([])
        },
        "web_sources_reviewed_present": web_sources_reviewed_present,
        "heading_count": heading_count,
        "paragraph_count": paragraph_count,
        "web_research_attempted": if use_upstream_evidence {
            json!(tool_telemetry.get("web_research_used").and_then(Value::as_bool).unwrap_or(false)
                || upstream_evidence.is_some_and(|evidence| evidence.web_research_attempted))
        } else {
            tool_telemetry.get("web_research_used").cloned().unwrap_or(json!(false))
        },
        "web_research_succeeded": if use_upstream_evidence {
            json!(tool_telemetry.get("web_research_succeeded").and_then(Value::as_bool).unwrap_or(false)
                || upstream_evidence.is_some_and(|evidence| evidence.web_research_succeeded))
        } else {
            tool_telemetry.get("web_research_succeeded").cloned().unwrap_or(json!(false))
        },
        "external_research_mode": external_research_mode,
        "upstream_evidence_applied": use_upstream_evidence,
        "upstream_notion_identity_unconfirmed": use_upstream_evidence
            && upstream_evidence.is_some_and(|evidence| evidence.notion_identity_unconfirmed),
        "upstream_external_citations_missing": use_upstream_evidence
            && upstream_evidence.is_some_and(|evidence| evidence.external_citations_missing),
        "blocked_handoff_cleanup_action": blocked_handoff_cleanup_action,
        "repair_attempted": repair_attempted,
        "repair_attempt": repair_attempt,
        "repair_attempts_remaining": repair_attempts_remaining,
        "repair_budget_spent": repair_attempt > 0,
        "repair_succeeded": repair_succeeded,
        "repair_exhausted": repair_exhausted,
        "validation_outcome": validation_outcome,
        "validation_profile": validation_profile,
        "validation_basis": validation_basis,
        "blocking_classification": blocking_classification,
        "required_next_tool_actions": required_next_tool_actions,
        "missing_required_mcp_tools": missing_required_concrete_mcp_tools,
        "unmet_requirements": unmet_requirements,
        "warning_requirements": warning_requirements.clone(),
        "warning_count": warning_requirements.len(),
        "artifact_candidates": artifact_candidates,
        "fintech_compliance_brief_validation": fintech_compliance_brief_validation,
        "resolved_enforcement": enforcement,
        "structured_handoff_present": structured_handoff.is_some(),
    });
    let rejected = metadata
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .and_then(|requirements| {
            if requirements.iter().any(|requirement| {
                requirement.as_str() == Some("provider_required_tool_mode_unsatisfied")
            }) {
                Some(
                    "artifact contains a provider required-tool/write-required failure marker"
                        .to_string(),
                )
            } else if requirements
                .iter()
                .any(|requirement| requirement.as_str() == Some("read_only_source_mutations"))
            {
                Some("read-only source-of-truth mutation detected".to_string())
            } else {
                None
            }
        })
        .or_else(|| {
            metadata
                .get("rejected_artifact_reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            metadata
                .get("semantic_block_reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    (accepted_output, metadata, rejected)
}

fn automation_node_requires_fintech_compliance_brief_validation(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
) -> bool {
    let fintech_strict = tandem_core::metadata_enables_fintech_strict(automation.metadata.as_ref())
        || tandem_core::metadata_enables_fintech_strict(node.metadata.as_ref());
    if !fintech_strict {
        return false;
    }
    node.output_contract
        .as_ref()
        .is_some_and(|contract| fintech_compliance_brief_marker(&contract.kind))
        || node
            .metadata
            .as_ref()
            .is_some_and(metadata_marks_fintech_compliance_brief)
}

fn metadata_marks_fintech_compliance_brief(metadata: &Value) -> bool {
    [
        metadata.get("artifact_contract"),
        metadata.get("artifact_type"),
        metadata.pointer("/fintech/artifact_contract"),
        metadata.pointer("/fintech/artifact_type"),
        metadata.pointer("/builder/artifact_contract"),
        metadata.pointer("/builder/artifact_type"),
    ]
    .into_iter()
    .flatten()
    .any(|value| {
        value.as_str().is_some_and(fintech_compliance_brief_marker)
            || value.as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.as_str().is_some_and(fintech_compliance_brief_marker))
            })
    })
}

fn fintech_compliance_brief_marker(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().replace('-', "_").as_str(),
        "fintech_compliance_brief"
            | "fintech_compliance_risk_brief"
            | "compliance_risk_update_brief"
            | "compliance_update_brief"
    )
}

fn session_fintech_connector_proof(
    session: &Session,
) -> Vec<tandem_core::FintechConnectorProofRecord> {
    let mut proof = Vec::new();
    for (message_index, message) in session.messages.iter().enumerate() {
        for (part_index, part) in message.parts.iter().enumerate() {
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
            let output = result.as_ref().map(Value::to_string);
            let record = tandem_core::build_tool_effect_ledger_record(
                "automation_validation",
                &format!("message-{message_index}-part-{part_index}"),
                None,
                tool,
                tandem_core::ToolEffectLedgerPhase::Outcome,
                tandem_core::ToolEffectLedgerStatus::Succeeded,
                args,
                result.as_ref(),
                output.as_deref(),
                None,
            );
            if let Some(row) = tandem_core::connector_proof_from_tool_record(&record) {
                proof.push(row);
            }
        }
    }
    proof
}

fn synthesis_overstates_unconfirmed_notion_identity(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("notion inspection artifact recorded that the target was the existing")
        || lowered.contains("upstream notion inspection artifact recorded that the target")
        || (lowered.contains("notion inspection")
            && lowered.contains("confirmed")
            && lowered.contains("existing"))
}

fn synthesis_makes_uncited_market_claims(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let acknowledges_no_external_evidence = lowered.contains("no current external web evidence")
        || lowered.contains("no direct web citations")
        || lowered.contains("web citations were unavailable")
        || lowered.contains("external web evidence was not collected");
    !acknowledges_no_external_evidence
        && (lowered.contains("market preference")
            || lowered.contains("market takeaways")
            || lowered.contains("safest market read")
            || lowered.contains("current market")
            || lowered.contains("vendor or source-by-source comparisons"))
}
pub(crate) fn parsed_status_u32(status: Option<&Value>, key: &str) -> Option<u32> {
    status
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

pub(crate) fn infer_artifact_repair_state(
    parsed_status: Option<&Value>,
    repair_attempted: bool,
    repair_succeeded: bool,
    semantic_block_reason: Option<&str>,
    tool_telemetry: &Value,
    repair_budget: Option<u32>,
) -> (u32, u32, bool) {
    let default_budget =
        repair_budget.unwrap_or_else(|| tandem_core::prewrite_repair_retry_max_attempts() as u32);
    let node_attempt = tool_telemetry_u32(tool_telemetry, "node_attempt");
    let node_max_attempts = tool_telemetry_u32(tool_telemetry, "node_max_attempts");
    let effective_budget = node_max_attempts
        .map(|max_attempts| max_attempts.saturating_sub(1))
        .map(|budget| budget.min(default_budget))
        .unwrap_or(default_budget);
    let inferred_attempt = tool_telemetry
        .get("tool_call_counts")
        .and_then(|value| value.get("write"))
        .and_then(Value::as_u64)
        .and_then(|count| count.checked_sub(1))
        .map(|count| count.min(effective_budget as u64) as u32)
        .unwrap_or(0);
    let node_repair_attempt = node_attempt
        .map(|attempt| attempt.saturating_sub(1))
        .unwrap_or(0);
    let repair_attempt = parsed_status_u32(parsed_status, "repairAttempt")
        .unwrap_or_else(|| {
            if repair_attempted {
                inferred_attempt.max(1)
            } else {
                inferred_attempt
            }
        })
        .max(node_repair_attempt)
        .min(effective_budget);
    let repair_attempts_remaining = parsed_status_u32(parsed_status, "repairAttemptsRemaining")
        .unwrap_or_else(|| effective_budget.saturating_sub(repair_attempt.min(effective_budget)));
    let repair_exhausted = parsed_status
        .and_then(|value| value.get("repairExhausted"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            let node_attempt_exhausted = node_attempt
                .zip(node_max_attempts)
                .is_some_and(|(attempt, max_attempts)| attempt >= max_attempts);
            node_attempt_exhausted
                || (repair_attempted
                    && !repair_succeeded
                    && semantic_block_reason.is_some()
                    && repair_attempt >= effective_budget)
        });
    (repair_attempt, repair_attempts_remaining, repair_exhausted)
}
