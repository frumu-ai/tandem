// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn value_string_array(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => text
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

pub fn normalize_connector_writer_metadata_value(metadata: &mut Value) -> bool {
    let Some(root) = metadata.as_object_mut() else {
        return false;
    };
    let has_notion_target = root.contains_key("notion_data_source_id")
        || root.contains_key("notion_data_source_url")
        || root.contains_key("notion_property_mappings")
        || root.contains_key("notion_default_properties");
    let explicit_writer = root
        .get("connector_writer")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let connector = root
        .get("connector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let connector_is_notion = has_notion_target || connector.as_deref() == Some("notion");
    if !explicit_writer && !has_notion_target {
        return false;
    }

    let mut changed = false;
    if !explicit_writer {
        root.insert("connector_writer".to_string(), Value::Bool(true));
        changed = true;
    }
    if connector.is_none() && has_notion_target {
        root.insert("connector".to_string(), Value::String("notion".to_string()));
        changed = true;
    }
    if !root.contains_key("connector_writer_contract_version") {
        root.insert(
            "connector_writer_contract_version".to_string(),
            Value::Number(1u64.into()),
        );
        changed = true;
    }
    if !root.contains_key("writer_kind") {
        root.insert(
            "writer_kind".to_string(),
            Value::String("database_rows".to_string()),
        );
        changed = true;
    }
    if connector_is_notion && !root.contains_key("update_after_create") {
        root.insert("update_after_create".to_string(), Value::Bool(true));
        changed = true;
    }
    if !root.contains_key("property_mappings") {
        if let Some(value) = root.get("notion_property_mappings").cloned() {
            root.insert("property_mappings".to_string(), value);
            changed = true;
        }
    }
    if !root.contains_key("default_properties") {
        if let Some(value) = root.get("notion_default_properties").cloned() {
            root.insert("default_properties".to_string(), value);
            changed = true;
        }
    }
    for key in ["duplicate_keys", "source_array_key", "source_array_path"] {
        if let Some(value) = root.get(key).cloned() {
            let values = value_string_array(&value);
            if !values.is_empty() && !matches!(value, Value::Array(_)) {
                if key == "source_array_key" || key == "source_array_path" {
                    if values.len() == 1 {
                        root.insert(key.to_string(), Value::String(values[0].clone()));
                    } else {
                        root.insert(
                            format!("{key}s"),
                            Value::Array(values.into_iter().map(Value::String).collect()),
                        );
                    }
                } else {
                    root.insert(
                        key.to_string(),
                        Value::Array(values.into_iter().map(Value::String).collect()),
                    );
                }
                changed = true;
            }
        }
    }
    changed
}

fn planner_mcp_server_segment(name: &str) -> String {
    name.trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

pub fn planner_connector_writer_contracts(server_tools: &[PlannerMcpServerToolSet]) -> Vec<Value> {
    let mut contracts = Vec::new();
    for server in server_tools {
        let segment = planner_mcp_server_segment(&server.server);
        let tools = server
            .tool_names
            .iter()
            .map(|tool| tool.trim().to_ascii_lowercase())
            .collect::<Vec<_>>();
        let has_notion_fetch = tools
            .iter()
            .any(|tool| tool == "mcp.notion.notion_fetch" || tool.ends_with(".notion_fetch"));
        let has_notion_search = tools
            .iter()
            .any(|tool| tool == "mcp.notion.notion_search" || tool.ends_with(".notion_search"));
        let has_notion_create = tools.iter().any(|tool| {
            tool == "mcp.notion.notion_create_pages" || tool.ends_with(".notion_create_pages")
        });
        let has_notion_update = tools.iter().any(|tool| {
            tool == "mcp.notion.notion_update_page" || tool.ends_with(".notion_update_page")
        });
        if segment == "notion"
            && has_notion_fetch
            && has_notion_search
            && has_notion_create
            && has_notion_update
        {
            contracts.push(json!({
                "connector": "notion",
                "writer_kind": "database_rows",
                "required_tools": [
                    "mcp.notion.notion_fetch",
                    "mcp.notion.notion_search",
                    "mcp.notion.notion_create_pages",
                    "mcp.notion.notion_update_page"
                ],
                "required_metadata": [
                    "connector",
                    "connector_writer",
                    "writer_kind",
                    "input_alias",
                    "source_array_key or source_array_path",
                    "notion_data_source_url or notion_data_source_id",
                    "title_property",
                    "property_mappings",
                    "duplicate_keys"
                ],
                "metadata_example": {
                    "connector": "notion",
                    "connector_writer": true,
                    "writer_kind": "database_rows",
                    "notion_data_source_url": "collection://...",
                    "input_alias": "filtered_rows",
                    "source_array_key": "rows",
                    "title_property": "Name",
                    "property_mappings": {"Name": "title", "URL": "url"},
                    "duplicate_keys": ["URL"]
                }
            }));
        }
    }
    contracts
}
