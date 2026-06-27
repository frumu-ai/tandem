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
