// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn automation_node_metadata_bool_local(node: &AutomationFlowNode, key: &str) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_metadata_value_local<'a>(
    node: &'a AutomationFlowNode,
    key: &str,
) -> Option<&'a Value> {
    node.metadata.as_ref().and_then(|metadata| metadata.get(key))
}

fn automation_node_metadata_string_local(node: &AutomationFlowNode, key: &str) -> Option<String> {
    automation_node_metadata_value_local(node, key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn automation_node_metadata_string_array_local(
    node: &AutomationFlowNode,
    key: &str,
) -> Vec<String> {
    match automation_node_metadata_value_local(node, key) {
        Some(Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

#[derive(Clone, Debug)]
struct AutomationNotionWriterConfig {
    data_source_id: String,
    data_source_url: String,
    source_node_id: String,
    input_alias: String,
    source_array_paths: Vec<String>,
    source_array_keys: Vec<String>,
    property_mappings: Vec<(String, Value)>,
    default_properties: serde_json::Map<String, Value>,
    duplicate_keys: Vec<String>,
    title_property: Option<String>,
    title_source: Option<Value>,
    update_after_create: bool,
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

fn automation_node_is_notion_connector_writer(node: &AutomationFlowNode) -> bool {
    automation_node_metadata_bool_local(node, "connector_writer")
        && automation_node_metadata_string_local(node, "connector")
            .as_deref()
            .map(|connector| connector.eq_ignore_ascii_case("notion"))
            .unwrap_or(true)
        && automation_notion_data_source_id(node).is_some()
        && automation_notion_data_source_url(node).is_some()
}

fn automation_notion_property_mappings(node: &AutomationFlowNode) -> Vec<(String, Value)> {
    for key in ["property_mappings", "notion_property_mappings"] {
        if let Some(Value::Object(object)) = automation_node_metadata_value_local(node, key) {
            return object
                .iter()
                .filter(|(property, _)| !property.trim().is_empty())
                .map(|(property, spec)| (property.clone(), spec.clone()))
                .collect();
        }
    }

    if let Some(Value::Array(items)) = automation_node_metadata_value_local(node, "properties") {
        return items
            .iter()
            .filter_map(|item| {
                let object = item.as_object()?;
                let property = object
                    .get("property")
                    .or_else(|| object.get("name"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                let spec = object
                    .get("mapping")
                    .or_else(|| object.get("source"))
                    .or_else(|| object.get("value"))
                    .cloned()
                    .unwrap_or(Value::Null);
                Some((property.to_string(), spec))
            })
            .collect();
    }

    Vec::new()
}

fn automation_notion_default_properties(
    node: &AutomationFlowNode,
) -> serde_json::Map<String, Value> {
    let mut defaults = serde_json::Map::new();
    for key in ["default_properties", "notion_default_properties"] {
        if let Some(Value::Object(object)) = automation_node_metadata_value_local(node, key) {
            for (property, value) in object {
                if !property.trim().is_empty() && !automation_notion_value_missing(value) {
                    defaults.insert(property.clone(), value.clone());
                }
            }
        }
    }

    if let Some(status_value) = automation_node_metadata_string_local(node, "status_property_value") {
        let status_property = automation_node_metadata_string_local(node, "status_property")
            .or_else(|| automation_node_metadata_string_local(node, "status_property_name"))
            .unwrap_or_else(|| "Status".to_string());
        defaults.entry(status_property).or_insert(json!(status_value));
    }
    defaults
}

fn automation_notion_writer_config(node: &AutomationFlowNode) -> Option<AutomationNotionWriterConfig> {
    if !automation_node_is_notion_connector_writer(node) {
        return None;
    }
    let data_source_id = automation_notion_data_source_id(node)?;
    let data_source_url = automation_notion_data_source_url(node)?;
    let source_node_id = automation_notion_source_node_id(node)?;
    let input_alias = automation_node_metadata_string_local(node, "input_alias")
        .or_else(|| automation_node_metadata_string_local(node, "source_input_alias"))
        .unwrap_or_else(|| "filtered_leads".to_string());
    let mut source_array_paths = automation_node_metadata_string_array_local(node, "source_array_paths");
    source_array_paths.extend(automation_node_metadata_string_array_local(
        node,
        "source_array_path",
    ));
    let mut source_array_keys = automation_node_metadata_string_array_local(node, "source_array_keys");
    source_array_keys.extend(automation_node_metadata_string_array_local(
        node,
        "source_array_key",
    ));
    if source_array_paths.is_empty() && source_array_keys.is_empty() {
        source_array_keys.extend(
            ["rows", "records", "items", "leads"]
                .into_iter()
                .map(str::to_string),
        );
    }
    let duplicate_keys = automation_node_metadata_string_array_local(node, "duplicate_keys");
    let update_after_create = automation_node_metadata_value_local(node, "update_after_create")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    Some(AutomationNotionWriterConfig {
        data_source_id,
        data_source_url,
        source_node_id,
        input_alias,
        source_array_paths,
        source_array_keys,
        property_mappings: automation_notion_property_mappings(node),
        default_properties: automation_notion_default_properties(node),
        duplicate_keys,
        title_property: automation_node_metadata_string_local(node, "title_property"),
        title_source: automation_node_metadata_value_local(node, "title_source").cloned(),
        update_after_create,
    })
}

fn automation_parse_json_text(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() || !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    serde_json::from_str::<Value>(trimmed).ok()
}

fn automation_notion_rows_from_candidate(
    value: &Value,
    config: &AutomationNotionWriterConfig,
) -> Option<Vec<Value>> {
    if let Value::Array(rows) = value {
        return Some(rows.clone());
    }
    if let Some(text) = value.as_str() {
        if let Some(parsed) = automation_parse_json_text(text) {
            return automation_notion_rows_from_candidate(&parsed, config);
        }
    }
    for pointer in &config.source_array_paths {
        if let Some(candidate) = value.pointer(pointer) {
            if let Some(rows) = automation_notion_rows_from_candidate(candidate, config) {
                return Some(rows);
            }
        }
    }
    for key in &config.source_array_keys {
        if let Some(rows) = value.get(key).and_then(Value::as_array) {
            return Some(rows.clone());
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
            if let Some(rows) = automation_notion_rows_from_candidate(candidate, config) {
                return Some(rows);
            }
        }
    }
    None
}

fn automation_notion_writer_rows(
    node: &AutomationFlowNode,
    upstream_inputs: &[Value],
    config: &AutomationNotionWriterConfig,
) -> Vec<Value> {
    for input in upstream_inputs {
        let alias_matches = input.get("alias").and_then(Value::as_str) == Some(config.input_alias.as_str());
        let source_matches = input
            .get("from_step_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value == config.source_node_id);
        if !(alias_matches || source_matches) {
            continue;
        }
        if let Some(output) = input.get("output") {
            if let Some(rows) = automation_notion_rows_from_candidate(output, config) {
                return rows;
            }
        }
    }

    for input in upstream_inputs {
        if let Some(output) = input.get("output") {
            if let Some(rows) = automation_notion_rows_from_candidate(output, config) {
                return rows;
            }
        }
    }

    if node.input_refs.is_empty() {
        return Vec::new();
    }
    Vec::new()
}

fn automation_notion_value_missing(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        _ => false,
    }
}

fn automation_notion_scalar_value(value: Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(json!(trimmed))
            }
        }
        Value::Bool(_) | Value::Number(_) => Some(value),
        Value::Array(_) | Value::Object(_) => Some(json!(value.to_string())),
    }
}

fn automation_notion_row_source_value(row: &Value, source: &str) -> Option<Value> {
    let source = source.trim();
    if source.is_empty() {
        return None;
    }
    if let Some(value) = row.get(source) {
        return Some(value.clone());
    }
    if source.starts_with('/') {
        return row.pointer(source).cloned();
    }
    let mut current = row;
    let mut found = false;
    for part in source.split('.') {
        if part.trim().is_empty() {
            return None;
        }
        let Some(next) = current.get(part) else {
            return None;
        };
        current = next;
        found = true;
    }
    if found {
        Some(current.clone())
    } else {
        None
    }
}

fn automation_notion_mapping_sources(spec: &Value) -> Vec<String> {
    match spec {
        Value::String(source) => vec![source.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Value::Object(object) => {
            if let Some(Value::Array(items)) = object.get("sources") {
                return items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            if let Some(Value::String(source)) = object.get("source") {
                return vec![source.clone()];
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn automation_notion_first_source_value(row: &Value, sources: &[String]) -> Option<Value> {
    for source in sources {
        let Some(value) = automation_notion_row_source_value(row, source) else {
            continue;
        };
        if !automation_notion_value_missing(&value) {
            return Some(value);
        }
    }
    None
}

fn automation_notion_apply_mapping_transforms(mut value: Value, spec: &Value) -> Option<Value> {
    let Some(object) = spec.as_object() else {
        return automation_notion_scalar_value(value);
    };

    if let Some(Value::Object(map)) = object.get("value_map") {
        if let Some(text) = value.as_str().map(str::trim) {
            if let Some(mapped) = map.get(text).or_else(|| map.get(&text.to_ascii_lowercase())) {
                value = mapped.clone();
            }
        }
    }

    if let Some(prefix) = object.get("prefix_if_missing").and_then(Value::as_str) {
        if let Some(text) = value.as_str().map(str::trim) {
            if !text.is_empty() && !text.starts_with(prefix) {
                value = json!(format!("{prefix}{text}"));
            }
        }
    }

    if let Some(Value::Array(allowed)) = object.get("allowed_values") {
        let allowed_values = allowed
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !allowed_values.is_empty() {
            if let Some(text) = value.as_str().map(str::trim) {
                if let Some(canonical) = allowed_values
                    .iter()
                    .find(|allowed| allowed.eq_ignore_ascii_case(text))
                {
                    value = json!(canonical);
                } else {
                    value = object
                        .get("fallback")
                        .or_else(|| object.get("default"))
                        .cloned()?;
                }
            } else {
                value = object
                    .get("fallback")
                    .or_else(|| object.get("default"))
                    .cloned()?;
            }
        }
    }

    automation_notion_scalar_value(value)
}

fn automation_notion_property_value(row: &Value, spec: &Value) -> Option<Value> {
    if let Value::Object(object) = spec {
        if let Some(value) = object.get("literal").or_else(|| object.get("value")) {
            return automation_notion_scalar_value(value.clone());
        }
        let sources = automation_notion_mapping_sources(spec);
        let value = automation_notion_first_source_value(row, &sources)
            .or_else(|| object.get("default").or_else(|| object.get("fallback")).cloned())?;
        return automation_notion_apply_mapping_transforms(value, spec);
    }

    let sources = automation_notion_mapping_sources(spec);
    automation_notion_first_source_value(row, &sources).and_then(automation_notion_scalar_value)
}

fn automation_notion_titleize_key(key: &str) -> String {
    key.split(['_', '-', ' '])
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!("{}{}", first.to_uppercase(), chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn automation_notion_inferred_property_mappings(row: &Value) -> Vec<(String, Value)> {
    row.as_object()
        .map(|object| {
            object
                .keys()
                .filter(|key| !key.trim().is_empty())
                .map(|key| (automation_notion_titleize_key(key), json!(key)))
                .collect()
        })
        .unwrap_or_default()
}

fn automation_notion_row_properties(
    row: &Value,
    config: &AutomationNotionWriterConfig,
) -> serde_json::Map<String, Value> {
    let mappings = if config.property_mappings.is_empty() {
        automation_notion_inferred_property_mappings(row)
    } else {
        config.property_mappings.clone()
    };
    let mut properties = serde_json::Map::new();
    for (property, spec) in mappings {
        let Some(value) = automation_notion_property_value(row, &spec) else {
            continue;
        };
        properties.insert(property, value);
    }
    for (property, value) in &config.default_properties {
        if !properties
            .get(property)
            .is_some_and(|value| !automation_notion_value_missing(value))
        {
            properties.insert(property.clone(), value.clone());
        }
    }
    properties
}

fn automation_notion_string_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
        _ => None,
    }
}

fn automation_notion_page_title(
    row: &Value,
    properties: &serde_json::Map<String, Value>,
    config: &AutomationNotionWriterConfig,
) -> String {
    if let Some(spec) = &config.title_source {
        if let Some(value) = automation_notion_property_value(row, spec)
            .and_then(|value| automation_notion_string_from_value(&value))
        {
            return value;
        }
    }
    if let Some(property) = &config.title_property {
        if let Some(value) = properties
            .get(property)
            .and_then(automation_notion_string_from_value)
        {
            return value;
        }
    }
    for value in properties.values() {
        if let Some(value) = automation_notion_string_from_value(value) {
            return value;
        }
    }
    "Notion row".to_string()
}

fn automation_tool_output_value(output: &str) -> Value {
    serde_json::from_str::<Value>(output).unwrap_or_else(|_| json!({ "text": output }))
}

fn automation_notion_tool_error(value: &Value) -> Option<String> {
    if value
        .get("isError")
        .and_then(Value::as_bool)
        .is_some_and(|is_error| is_error)
    {
        return Some(value.to_string());
    }
    if value
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|text| {
            text.starts_with("MCP error")
                || text.contains("Input validation error")
                || text.contains("Invalid arguments for tool")
        })
    {
        return Some(value.to_string());
    }
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

fn automation_connector_tool_error_retryable(error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();
    if lowered.contains("input validation error") || lowered.contains("invalid arguments for tool") {
        return false;
    }

    [
        "error sending request",
        "failed to read mcp response",
        "error decoding response body",
        "request timed out",
        "timed out",
        "timeout",
        "connection reset",
        "connection closed",
        "connection refused",
        "connection aborted",
        "temporarily unavailable",
        "service unavailable",
        "bad gateway",
        "gateway timeout",
        "too many requests",
        "rate limit",
        "http 429",
        "http 502",
        "http 503",
        "http 504",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn automation_connector_tool_max_attempts(tool: &str) -> usize {
    if tool.starts_with("mcp.") {
        3
    } else {
        1
    }
}

fn automation_connector_retry_delay(attempt: usize) -> Duration {
    match attempt {
        1 => Duration::from_millis(250),
        2 => Duration::from_millis(1000),
        _ => Duration::from_millis(2000),
    }
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
    let max_attempts = automation_connector_tool_max_attempts(tool);
    let mut last_error = None::<String>;

    for attempt in 1..=max_attempts {
        let dispatch_context = state.tool_dispatch_context(
            tandem_tools::ToolDispatchSource::new("automation_notion_writer")
                .run(run_id)
                .node(node.node_id.clone()),
            tenant_context.clone(),
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
                let retryable = semantic_error
                    .as_deref()
                    .is_some_and(automation_connector_tool_error_retryable);
                let will_retry = retryable && attempt < max_attempts;
                invocation_parts.push(MessagePart::ToolInvocation {
                    tool: tool.to_string(),
                    args: args.clone(),
                    result: Some(result_value.clone()),
                    error: None,
                });
                call_rows.push(json!({
                    "tool": tool,
                    "args": args,
                    "attempt": attempt,
                    "max_attempts": max_attempts,
                    "status": if semantic_error.is_some() {
                        if will_retry { "retrying" } else { "failed" }
                    } else {
                        "completed"
                    },
                    "result_excerpt": truncate_text(&result_value.to_string(), 1600),
                    "error": semantic_error,
                    "retryable": retryable,
                }));
                if let Some(error) = semantic_error {
                    if will_retry {
                        last_error = Some(error);
                        tokio::time::sleep(automation_connector_retry_delay(attempt)).await;
                        continue;
                    }
                    return Err(error);
                }
                return Ok(parsed_output);
            }
            Err(error) => {
                let error_text = error.to_string();
                let retryable = automation_connector_tool_error_retryable(&error_text);
                let will_retry = retryable && attempt < max_attempts;
                invocation_parts.push(MessagePart::ToolInvocation {
                    tool: tool.to_string(),
                    args: args.clone(),
                    result: None,
                    error: Some(error_text.clone()),
                });
                call_rows.push(json!({
                    "tool": tool,
                    "args": args,
                    "attempt": attempt,
                    "max_attempts": max_attempts,
                    "status": if will_retry { "retrying" } else { "failed" },
                    "error": error_text,
                    "retryable": retryable,
                }));
                if will_retry {
                    last_error = Some(error_text);
                    tokio::time::sleep(automation_connector_retry_delay(attempt)).await;
                    continue;
                }
                return Err(error_text);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| format!("{tool} failed without returning an output")))
}

fn automation_notion_duplicate_queries(
    properties: &serde_json::Map<String, Value>,
    config: &AutomationNotionWriterConfig,
) -> Vec<(String, String)> {
    config
        .duplicate_keys
        .iter()
        .filter_map(|property| {
            let value = properties
                .get(property)
                .and_then(automation_notion_string_from_value)?;
            Some((property.clone(), value))
        })
        .collect()
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
    let Some(config) = automation_notion_writer_config(node) else {
        return Ok(None);
    };
    let Some(output_path) = required_output_path else {
        return Ok(None);
    };

    let resolved_output =
        resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    if let Some(parent) = resolved_output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let rows = automation_notion_writer_rows(node, upstream_inputs, &config);
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
    let mut inserted_row_keys = Vec::<Value>::new();
    let mut updated_page_ids = Vec::<String>::new();
    let mut warnings = Vec::<String>::new();
    let mut inserted_count = 0usize;
    let mut skipped_duplicate_count = 0usize;
    let mut failed_count = 0usize;

    if !rows.is_empty() {
        if let Err(error) = automation_dispatch_deterministic_tool(
            state,
            run_id,
            node,
            tenant_context.clone(),
            &dispatch_scope,
            "mcp.notion.notion_fetch",
            json!({ "id": config.data_source_url.clone() }),
            &mut invocation_parts,
            &mut call_rows,
        )
        .await
        {
            failed_count = rows.len();
            errors.push(format!(
                "notion_fetch failed for {}: {error}",
                config.data_source_url
            ));
        } else {
            for row in &rows {
                let properties = automation_notion_row_properties(row, &config);
                if properties.is_empty() {
                    failed_count += 1;
                    errors.push(format!(
                        "no Notion properties could be mapped for row `{}`",
                        truncate_text(&row.to_string(), 400)
                    ));
                    continue;
                }
                let title = automation_notion_page_title(row, &properties, &config);
                let duplicate_queries = automation_notion_duplicate_queries(&properties, &config);
                let row_key = json!({
                    "duplicate_values": duplicate_queries
                        .iter()
                        .map(|(property, value)| json!({ "property": property, "value": value }))
                        .collect::<Vec<_>>(),
                    "title": title,
                });
                let mut duplicate_ref = None;
                let mut duplicate_search_errors = Vec::<String>::new();
                for (property, query) in &duplicate_queries {
                    match automation_dispatch_deterministic_tool(
                        state,
                        run_id,
                        node,
                        tenant_context.clone(),
                        &dispatch_scope,
                        "mcp.notion.notion_search",
                        json!({
                            "query": query,
                            "data_source_url": config.data_source_url.clone(),
                            "page_size": 5,
                        }),
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
                        Err(error) => duplicate_search_errors.push(format!(
                            "notion_search failed for `{query}` ({property}) from `{}`: {error}",
                            config.source_node_id
                        )),
                    }
                }

                if let Some(page_ref) = duplicate_ref {
                    warnings.extend(duplicate_search_errors);
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
                            errors.push(format!(
                                "notion_update_page failed for duplicate row `{}`: {error}",
                                truncate_text(&row_key.to_string(), 400)
                            ));
                        }
                    }
                    continue;
                }

                if !duplicate_search_errors.is_empty() {
                    failed_count += 1;
                    errors.extend(duplicate_search_errors.into_iter().map(|error| {
                        format!(
                            "{error}; duplicate detection was incomplete, so row creation was skipped"
                        )
                    }));
                    continue;
                }

                let create_args = json!({
                    "parent": {
                        "type": "data_source_id",
                        "data_source_id": config.data_source_id.clone(),
                    },
                    "pages": [
                        {
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
                                "notion_create_pages did not return a page id/url for row `{}`",
                                truncate_text(&row_key.to_string(), 400)
                            ));
                            continue;
                        };
                        if config.update_after_create {
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
                                    "properties": automation_notion_row_properties(row, &config),
                                }),
                                &mut invocation_parts,
                                &mut call_rows,
                            )
                            .await
                            {
                                Ok(_) => {
                                    inserted_count += 1;
                                    inserted_row_keys.push(row_key.clone());
                                    updated_page_ids.push(created_ref);
                                }
                                Err(error) => {
                                    failed_count += 1;
                                    errors.push(format!(
                                        "notion_update_page failed for created row `{}`: {error}",
                                        truncate_text(&row_key.to_string(), 400)
                                    ));
                                }
                            }
                        } else {
                            inserted_count += 1;
                            inserted_row_keys.push(row_key.clone());
                            updated_page_ids.push(created_ref);
                        }
                    }
                    Err(error) => {
                        failed_count += 1;
                        errors.push(format!(
                            "notion_create_pages failed for row `{}`: {error}",
                            truncate_text(&row_key.to_string(), 400)
                        ));
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
        "inserted_row_keys": inserted_row_keys,
        "updated_page_ids": updated_page_ids,
        "errors": errors,
        "warnings": warnings,
        "notion_data_source_url": config.data_source_url,
        "source_node_id": config.source_node_id,
        "input_alias": config.input_alias,
        "duplicate_keys": config.duplicate_keys,
        "mapped_properties": config
            .property_mappings
            .iter()
            .map(|(property, _)| property.clone())
            .collect::<Vec<_>>(),
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

#[cfg(test)]
mod deterministic_notion_writer_tests {
    use super::*;

    #[test]
    fn notion_tool_error_detects_plain_text_mcp_validation_errors() {
        let error = automation_notion_tool_error(&json!({
            "text": "MCP error -32602: Input validation error: Invalid arguments for tool notion-create-pages"
        }))
        .expect("plain text MCP validation error should be treated as a failed tool call");

        assert!(error.contains("notion-create-pages"));
    }

    #[test]
    fn connector_tool_retryable_error_classifier_excludes_schema_errors() {
        assert!(automation_connector_tool_error_retryable(
            "MCP request failed: error sending request for url (https://mcp.notion.com/mcp)"
        ));
        assert!(automation_connector_tool_error_retryable(
            "HTTP 503 service unavailable while calling connector"
        ));
        assert!(automation_connector_tool_error_retryable(
            "Failed to read MCP response: error decoding response body"
        ));
        assert!(!automation_connector_tool_error_retryable(
            "MCP error -32602: Input validation error: Invalid arguments for tool notion-create-pages"
        ));
    }

    #[test]
    fn notion_mapping_allowed_values_preserve_canonical_case() {
        let spec = json!({
            "sources": ["subreddit"],
            "prefix_if_missing": "r/",
            "allowed_values": ["r/LocalLLaMA", "r/sysadmin", "r/mcp", "r/DevOps"],
            "fallback": "r/LocalLLaMA"
        });

        let mapped = automation_notion_property_value(&json!({ "subreddit": "devops" }), &spec)
            .expect("subreddit should map to a canonical allowed value");

        assert_eq!(mapped, json!("r/DevOps"));
    }
}
