// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use tandem_plan_compiler::api as compiler_api;
use tandem_types::TenantContext;

use super::AppState;

fn connector_writer_step_alias_from_step_id(step_id: &str) -> String {
    let alias = step_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let alias = alias.trim_matches('_').replace("__", "_");
    if alias.is_empty() {
        "upstream_artifact".to_string()
    } else if alias.ends_with("_artifact") {
        alias
    } else {
        format!("{alias}_artifact")
    }
}

fn connector_writer_truncate_text(input: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_len {
            break;
        }
        end = next;
    }
    let mut out = input[..end].to_string();
    out.push_str("...");
    out
}

#[derive(Clone, Debug, Default)]
struct NotionPropertySchema {
    property_type: Option<String>,
    options: BTreeSet<String>,
}

#[derive(Clone, Debug, Default)]
struct NotionDataSourceSchema {
    data_source_id: String,
    data_source_url: String,
    title_property: Option<String>,
    properties: BTreeMap<String, NotionPropertySchema>,
}

fn workflow_metadata_string(metadata: Option<&Value>, key: &str) -> Option<String> {
    metadata
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn workflow_metadata_bool(metadata: Option<&Value>, key: &str) -> bool {
    metadata
        .and_then(|value| value.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn workflow_metadata_string_array(metadata: Option<&Value>, key: &str) -> Vec<String> {
    match metadata.and_then(|value| value.get(key)) {
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn workflow_metadata_object_mut(step: &mut crate::WorkflowPlanStep) -> &mut Map<String, Value> {
    let metadata = step.metadata.get_or_insert_with(|| json!({}));
    if !metadata.is_object() {
        *metadata = json!({});
    }
    metadata.as_object_mut().expect("metadata object")
}

fn workflow_plan_allows_notion(plan: &crate::WorkflowPlan) -> bool {
    plan.allowed_mcp_servers.iter().any(|server| {
        let server = server.trim().to_ascii_lowercase();
        server == "notion" || server == "mcp.notion" || server.contains("notion")
    })
}

fn workflow_text_mentions_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn workflow_value_contains_text(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text.to_ascii_lowercase().contains(needle),
        Value::Array(items) => items
            .iter()
            .any(|item| workflow_value_contains_text(item, needle)),
        Value::Object(object) => object.iter().any(|(key, value)| {
            key.to_ascii_lowercase().contains(needle) || workflow_value_contains_text(value, needle)
        }),
        _ => false,
    }
}

fn workflow_step_is_notion_database_writer_candidate(step: &crate::WorkflowPlanStep) -> bool {
    let metadata = step.metadata.as_ref();
    let connector_is_notion = workflow_metadata_string(metadata, "connector")
        .as_deref()
        .is_some_and(|connector| connector.eq_ignore_ascii_case("notion"));
    let has_notion_writer_metadata = connector_is_notion
        || workflow_metadata_bool(metadata, "connector_writer")
        || workflow_metadata_string(metadata, "notion_data_source_id").is_some()
        || workflow_metadata_string(metadata, "notion_data_source_url").is_some()
        || metadata
            .and_then(|value| value.get("notion_property_mappings"))
            .is_some();
    if has_notion_writer_metadata {
        return true;
    }

    let text = format!(
        "{}\n{}\n{}\n{}",
        step.step_id, step.kind, step.objective, step.agent_role
    )
    .to_ascii_lowercase();
    let mentions_notion = text.contains("notion")
        || workflow_value_contains_text(metadata.unwrap_or(&Value::Null), "mcp.notion");
    let mentions_database = workflow_text_mentions_any(
        &text,
        &[
            "database",
            "data source",
            "data_source",
            "table",
            "row",
            "rows",
            "record",
            "records",
        ],
    );
    let mentions_write = workflow_text_mentions_any(
        &text,
        &[
            "write", "save", "insert", "append", "create", "update", "upsert", "add ",
        ],
    );
    mentions_notion && mentions_database && mentions_write
}

fn workflow_clean_target_token(token: &str) -> String {
    token
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '"' | '\'' | '<' | '>' | '[' | ']' | '(' | ')' | ',' | '.' | ';' | ':'
            )
        })
        .to_string()
}

fn workflow_extract_notion_target_from_text(text: &str) -> Option<String> {
    for raw in text.split_whitespace() {
        let token = workflow_clean_target_token(raw);
        if token.starts_with("collection://")
            || token.contains("notion.so/")
            || token.contains("notion.site/")
        {
            return Some(token);
        }
    }
    None
}

fn workflow_notion_writer_target(
    plan: &crate::WorkflowPlan,
    step: &crate::WorkflowPlanStep,
) -> Option<String> {
    let metadata = step.metadata.as_ref();
    for key in [
        "notion_data_source_url",
        "notion_data_source_id",
        "data_source_url",
        "data_source_id",
        "database_url",
        "database_id",
    ] {
        if let Some(value) = workflow_metadata_string(metadata, key) {
            return Some(value);
        }
    }
    let text = format!("{}\n{}", plan.original_prompt, step.objective);
    workflow_extract_notion_target_from_text(&text)
}

fn workflow_notion_id_from_target(target: &str) -> String {
    target
        .trim()
        .strip_prefix("collection://")
        .unwrap_or_else(|| target.trim())
        .to_string()
}

fn workflow_notion_url_from_target(target: &str) -> String {
    let target = target.trim();
    if target.starts_with("collection://")
        || target.starts_with("http://")
        || target.starts_with("https://")
    {
        target.to_string()
    } else {
        format!("collection://{target}")
    }
}

fn notion_property_type(value: &Value) -> Option<String> {
    if let Some(kind) = value
        .get("type")
        .or_else(|| value.get("property_type"))
        .or_else(|| value.get("kind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(kind.to_ascii_lowercase());
    }
    for key in [
        "title",
        "rich_text",
        "url",
        "select",
        "status",
        "date",
        "number",
        "email",
        "phone_number",
        "checkbox",
    ] {
        if value.get(key).is_some() {
            return Some(key.to_string());
        }
    }
    None
}

fn collect_notion_option_names(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(name) = object
                .get("name")
                .or_else(|| object.get("label"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                out.insert(name.to_string());
            }
            for value in object.values() {
                collect_notion_option_names(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_notion_option_names(item, out);
            }
        }
        Value::String(text) => {
            let text = text.trim();
            if !text.is_empty() {
                out.insert(text.to_string());
            }
        }
        _ => {}
    }
}

fn notion_schema_object_id(object: &Map<String, Value>, explicit_target: Option<&str>) -> String {
    for key in ["data_source_id", "database_id", "id"] {
        if let Some(value) = object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return workflow_notion_id_from_target(value);
        }
    }
    explicit_target
        .map(workflow_notion_id_from_target)
        .unwrap_or_default()
}

fn notion_schema_object_url(object: &Map<String, Value>, explicit_target: Option<&str>) -> String {
    for key in ["data_source_url", "database_url", "url"] {
        if let Some(value) = object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return workflow_notion_url_from_target(value);
        }
    }
    explicit_target
        .map(workflow_notion_url_from_target)
        .unwrap_or_default()
}

fn extract_notion_schema_from_object(
    object: &Map<String, Value>,
    explicit_target: Option<&str>,
) -> Option<NotionDataSourceSchema> {
    let properties = object
        .get("properties")
        .or_else(|| object.get("schema"))
        .or_else(|| object.get("property_schema"))
        .or_else(|| object.get("properties_schema"))
        .and_then(Value::as_object)?;
    let mut schema = NotionDataSourceSchema {
        data_source_id: notion_schema_object_id(object, explicit_target),
        data_source_url: notion_schema_object_url(object, explicit_target),
        title_property: object
            .get("title_property")
            .or_else(|| object.get("titleProperty"))
            .or_else(|| object.get("title_property_name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        properties: BTreeMap::new(),
    };
    for (fallback_name, property) in properties {
        let name = property
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback_name)
            .to_string();
        let property_type = notion_property_type(property);
        let mut options = BTreeSet::new();
        if property_type
            .as_deref()
            .is_some_and(|kind| kind == "select" || kind == "status")
        {
            collect_notion_option_names(property, &mut options);
            options.remove(&name);
        }
        if property_type.as_deref() == Some("title") && schema.title_property.is_none() {
            schema.title_property = Some(name.clone());
        }
        schema.properties.insert(
            name,
            NotionPropertySchema {
                property_type,
                options,
            },
        );
    }
    if schema.properties.is_empty() {
        return None;
    }
    if schema.data_source_id.is_empty() {
        schema.data_source_id = explicit_target
            .map(workflow_notion_id_from_target)
            .unwrap_or_default();
    }
    if schema.data_source_url.is_empty() {
        schema.data_source_url = explicit_target
            .map(workflow_notion_url_from_target)
            .unwrap_or_else(|| workflow_notion_url_from_target(&schema.data_source_id));
    }
    Some(schema)
}

fn extract_notion_data_source_schema(
    value: &Value,
    explicit_target: Option<&str>,
) -> Option<NotionDataSourceSchema> {
    match value {
        Value::Object(object) => {
            if let Some(schema) = extract_notion_schema_from_object(object, explicit_target) {
                return Some(schema);
            }
            for value in object.values() {
                if let Some(schema) = extract_notion_data_source_schema(value, explicit_target) {
                    return Some(schema);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(schema) = extract_notion_data_source_schema(item, explicit_target) {
                    return Some(schema);
                }
            }
            None
        }
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|parsed| extract_notion_data_source_schema(&parsed, explicit_target)),
        _ => None,
    }
}

fn notion_tool_result_value(output: &str, metadata: &Value) -> Value {
    let output_value = serde_json::from_str::<Value>(output.trim())
        .unwrap_or_else(|_| Value::String(output.to_string()));
    json!({
        "output": output_value,
        "metadata": metadata,
    })
}

async fn fetch_notion_data_source_schema(
    state: &AppState,
    target: &str,
) -> Result<NotionDataSourceSchema, String> {
    let dispatch_context = state.tool_dispatch_context(
        tandem_tools::ToolDispatchSource::new("workflow_planner_connector_writer"),
        TenantContext::local_implicit(),
        vec![
            "mcp.notion.*".to_string(),
            "mcp.notion.notion_fetch".to_string(),
            "mcp.notion.notion_search".to_string(),
        ],
    );
    let result = state
        .tool_dispatcher
        .dispatch(
            "mcp.notion.notion_fetch",
            json!({ "id": workflow_notion_url_from_target(target) }),
            dispatch_context,
        )
        .await
        .map_err(|error| {
            format!(
                "Notion data source schema preflight failed for `{}`: {}",
                target,
                connector_writer_truncate_text(&error.to_string(), 500)
            )
        })?;
    let payload = notion_tool_result_value(&result.output, &result.metadata);
    extract_notion_data_source_schema(&payload, Some(target)).ok_or_else(|| {
        format!(
            "Notion data source `{}` was fetched, but Tandem could not find a database schema in the MCP response. Select an existing Notion database/data source and try again.",
            target
        )
    })
}

fn source_name_for_property(property: &str) -> String {
    let mut out = String::new();
    let mut previous_was_sep = false;
    for ch in property.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_was_sep = false;
        } else if !previous_was_sep {
            out.push('_');
            previous_was_sep = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn metadata_property_mappings(metadata: Option<&Value>) -> BTreeMap<String, Value> {
    metadata
        .and_then(|metadata| {
            metadata
                .get("property_mappings")
                .or_else(|| metadata.get("notion_property_mappings"))
        })
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter(|(key, _)| !key.trim().is_empty())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn metadata_default_properties(metadata: Option<&Value>) -> BTreeMap<String, Value> {
    metadata
        .and_then(|metadata| {
            metadata
                .get("default_properties")
                .or_else(|| metadata.get("notion_default_properties"))
        })
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter(|(key, _)| !key.trim().is_empty())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn infer_notion_property_mappings(schema: &NotionDataSourceSchema) -> BTreeMap<String, Value> {
    schema
        .properties
        .keys()
        .map(|property| {
            (
                property.clone(),
                Value::String(source_name_for_property(property)),
            )
        })
        .collect()
}

fn choose_notion_duplicate_keys(
    schema: &NotionDataSourceSchema,
    mappings: &BTreeMap<String, Value>,
    defaults: &BTreeMap<String, Value>,
) -> Vec<String> {
    for preferred in [
        "Thread Link",
        "URL",
        "Url",
        "Link",
        "Email",
        "User Handle",
        "Handle",
    ] {
        if schema.properties.contains_key(preferred)
            && (mappings.contains_key(preferred) || defaults.contains_key(preferred))
        {
            return vec![preferred.to_string()];
        }
    }
    if let Some(title) = schema.title_property.as_ref() {
        if mappings.contains_key(title) || defaults.contains_key(title) {
            return vec![title.clone()];
        }
    }
    Vec::new()
}

fn notion_static_mapping_values(spec: &Value) -> Vec<String> {
    let mut values = Vec::new();
    if let Value::Object(object) = spec {
        for key in ["literal", "default", "const"] {
            if let Some(value) = object.get(key).and_then(Value::as_str) {
                let value = value.trim();
                if !value.is_empty() {
                    values.push(value.to_string());
                }
            }
        }
    }
    values
}

fn notion_scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_string()).filter(|value| !value.is_empty()),
        Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
        _ => None,
    }
}

fn validate_notion_option_value(
    property: &str,
    value: &str,
    schema_property: &NotionPropertySchema,
) -> Result<(), String> {
    if !schema_property
        .property_type
        .as_deref()
        .is_some_and(|kind| kind == "select" || kind == "status")
        || schema_property.options.is_empty()
    {
        return Ok(());
    }
    if schema_property.options.contains(value) {
        return Ok(());
    }
    Err(format!(
        "Notion property `{property}` value `{value}` is not one of the schema options: {}",
        schema_property
            .options
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn enrich_notion_connector_writer_step_from_schema(
    step: &mut crate::WorkflowPlanStep,
    schema: &NotionDataSourceSchema,
) -> Result<Value, String> {
    if schema.properties.is_empty() {
        return Err("Notion database writer preflight requires a non-empty schema.".to_string());
    }
    let title_property = workflow_metadata_string(step.metadata.as_ref(), "title_property")
        .or_else(|| schema.title_property.clone())
        .ok_or_else(|| {
            "Notion database writer preflight could not determine the schema title property."
                .to_string()
        })?;
    if !schema.properties.contains_key(&title_property) {
        return Err(format!(
            "Notion title_property `{}` is not present in the resolved schema.",
            title_property
        ));
    }

    let mut mappings = metadata_property_mappings(step.metadata.as_ref());
    if mappings.is_empty() {
        mappings = infer_notion_property_mappings(schema);
    }
    if mappings.is_empty() {
        return Err(
            "Notion database writer preflight requires property_mappings for at least one schema property."
                .to_string(),
        );
    }
    let defaults = metadata_default_properties(step.metadata.as_ref());
    for property in mappings.keys().chain(defaults.keys()) {
        if !schema.properties.contains_key(property) {
            return Err(format!(
                "Notion writer property `{}` is not present in the resolved schema.",
                property
            ));
        }
    }

    for (property, value) in &defaults {
        if let Some(schema_property) = schema.properties.get(property) {
            if let Some(value) = notion_scalar_string(value) {
                validate_notion_option_value(property, &value, schema_property)?;
            }
        }
    }
    for (property, spec) in &mappings {
        if let Some(schema_property) = schema.properties.get(property) {
            for value in notion_static_mapping_values(spec) {
                validate_notion_option_value(property, &value, schema_property)?;
            }
        }
    }

    let mut duplicate_keys =
        workflow_metadata_string_array(step.metadata.as_ref(), "duplicate_keys");
    let append_only = workflow_metadata_string(step.metadata.as_ref(), "duplicate_policy")
        .as_deref()
        .is_some_and(|policy| policy.eq_ignore_ascii_case("append_only"));
    if duplicate_keys.is_empty() && !append_only {
        duplicate_keys = choose_notion_duplicate_keys(schema, &mappings, &defaults);
    }
    if duplicate_keys.is_empty() && !append_only {
        return Err(
            "Notion database writer preflight requires duplicate_keys or duplicate_policy=append_only."
                .to_string(),
        );
    }
    for key in &duplicate_keys {
        if !schema.properties.contains_key(key) {
            return Err(format!(
                "Notion duplicate key `{}` is not present in the resolved schema.",
                key
            ));
        }
        if !mappings.contains_key(key) && !defaults.contains_key(key) {
            return Err(format!(
                "Notion duplicate key `{}` must also be mapped or defaulted so duplicate search has a value.",
                key
            ));
        }
    }

    let mut input_alias = workflow_metadata_string(step.metadata.as_ref(), "input_alias");
    if input_alias.is_none() {
        input_alias = step.input_refs.first().map(|input| input.alias.clone());
    }
    if input_alias.is_none() && step.depends_on.len() == 1 {
        input_alias = Some(connector_writer_step_alias_from_step_id(
            &step.depends_on[0],
        ));
    }
    let input_alias = input_alias.ok_or_else(|| {
        "Notion database writer preflight requires input_alias and an upstream input_ref."
            .to_string()
    })?;
    let has_matching_input_ref = step
        .input_refs
        .iter()
        .any(|input| input.alias == input_alias || input.from_step_id == input_alias);
    if !has_matching_input_ref {
        if step.depends_on.len() == 1 {
            step.input_refs.push(crate::AutomationFlowInputRef {
                from_step_id: step.depends_on[0].clone(),
                alias: input_alias.clone(),
            });
        } else {
            return Err(format!(
                "Notion writer input_alias `{}` does not match an input_ref, and the writer has {} dependencies.",
                input_alias,
                step.depends_on.len()
            ));
        }
    }
    let source_node_id = step
        .input_refs
        .iter()
        .find(|input| input.alias == input_alias)
        .map(|input| input.from_step_id.clone())
        .or_else(|| {
            step.input_refs
                .first()
                .map(|input| input.from_step_id.clone())
        })
        .ok_or_else(|| "Notion writer needs a concrete upstream source_node_id.".to_string())?;

    let has_source_array = workflow_metadata_string(step.metadata.as_ref(), "source_array_key")
        .is_some()
        || workflow_metadata_string(step.metadata.as_ref(), "source_array_path").is_some()
        || !workflow_metadata_string_array(step.metadata.as_ref(), "source_array_keys").is_empty()
        || !workflow_metadata_string_array(step.metadata.as_ref(), "source_array_paths").is_empty();

    let root = workflow_metadata_object_mut(step);
    root.insert("connector".to_string(), Value::String("notion".to_string()));
    root.insert("connector_writer".to_string(), Value::Bool(true));
    root.insert(
        "connector_writer_contract_version".to_string(),
        Value::Number(1u64.into()),
    );
    root.insert(
        "writer_kind".to_string(),
        Value::String("database_rows".to_string()),
    );
    root.insert(
        "notion_data_source_id".to_string(),
        Value::String(schema.data_source_id.clone()),
    );
    root.insert(
        "notion_data_source_url".to_string(),
        Value::String(schema.data_source_url.clone()),
    );
    root.insert("input_alias".to_string(), Value::String(input_alias));
    root.insert("source_node_id".to_string(), Value::String(source_node_id));
    if !has_source_array {
        root.insert(
            "source_array_key".to_string(),
            Value::String("rows".to_string()),
        );
    }
    root.insert("title_property".to_string(), Value::String(title_property));
    root.insert(
        "property_mappings".to_string(),
        Value::Object(mappings.into_iter().collect()),
    );
    if !defaults.is_empty() {
        root.insert(
            "default_properties".to_string(),
            Value::Object(defaults.into_iter().collect()),
        );
    }
    if !duplicate_keys.is_empty() {
        root.insert(
            "duplicate_keys".to_string(),
            Value::Array(duplicate_keys.into_iter().map(Value::String).collect()),
        );
    }
    root.insert("update_after_create".to_string(), Value::Bool(true));
    if let Some(metadata) = step.metadata.as_mut() {
        compiler_api::normalize_connector_writer_metadata_value(metadata);
    }
    Ok(json!({
        "node_id": step.step_id,
        "connector": "notion",
        "writer_kind": "database_rows",
        "notion_data_source_url": schema.data_source_url,
        "property_count": schema.properties.len(),
    }))
}

pub(crate) async fn resolve_workflow_plan_connector_writers(
    state: &AppState,
    plan: &mut crate::WorkflowPlan,
) -> Result<Option<Value>, String> {
    let candidate_indices = plan
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            if workflow_step_is_notion_database_writer_candidate(step) {
                Some(index)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if candidate_indices.is_empty() {
        return Ok(None);
    }
    if !workflow_plan_allows_notion(plan) {
        return Err(
            "This workflow contains a Notion database writer step, but the Notion MCP server is not selected for the workflow."
                .to_string(),
        );
    }

    let mut rows = Vec::new();
    for index in candidate_indices {
        let step_id = plan.steps[index].step_id.clone();
        let target = workflow_notion_writer_target(plan, &plan.steps[index]).ok_or_else(|| {
            format!(
                "Notion database writer step `{}` needs notion_data_source_url or notion_data_source_id before the workflow can be saved.",
                step_id
            )
        })?;
        let metadata_schema = plan.steps[index]
            .metadata
            .as_ref()
            .and_then(|metadata| {
                metadata
                    .get("notion_schema")
                    .or_else(|| metadata.get("data_source_schema"))
                    .or_else(|| metadata.get("notion_data_source_schema"))
            })
            .and_then(|value| extract_notion_data_source_schema(value, Some(&target)));
        let schema = match metadata_schema {
            Some(schema) => schema,
            None => fetch_notion_data_source_schema(state, &target).await?,
        };
        let row = enrich_notion_connector_writer_step_from_schema(&mut plan.steps[index], &schema)
            .map_err(|error| {
                format!("Notion database writer step `{step_id}` is invalid: {error}")
            })?;
        rows.push(row);
    }
    Ok(Some(json!({
        "connector_writer_resolution": {
            "status": "validated",
            "validated_writers": rows,
        }
    })))
}

pub(crate) fn merge_planner_diagnostics(target: &mut Option<Value>, addition: Value) {
    if addition.is_null() {
        return;
    }
    if let (Some(Value::Object(existing)), Some(addition)) = (target.as_mut(), addition.as_object())
    {
        for (key, value) in addition {
            existing.insert(key.clone(), value.clone());
        }
        return;
    }
    match target.take() {
        None => {
            *target = Some(addition);
        }
        Some(previous) => {
            *target = Some(json!({
                "previous_diagnostics": previous,
                "connector_writer_resolution": addition,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_notion_schema() -> NotionDataSourceSchema {
        let mut properties = BTreeMap::new();
        properties.insert(
            "Topic / Thread Title".to_string(),
            NotionPropertySchema {
                property_type: Some("title".to_string()),
                options: BTreeSet::new(),
            },
        );
        properties.insert(
            "Thread Link".to_string(),
            NotionPropertySchema {
                property_type: Some("url".to_string()),
                options: BTreeSet::new(),
            },
        );
        properties.insert(
            "Status".to_string(),
            NotionPropertySchema {
                property_type: Some("status".to_string()),
                options: ["New Lead", "Reviewed", "Outreach Sent", "Pilot Engaged"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            },
        );
        NotionDataSourceSchema {
            data_source_id: "ds-123".to_string(),
            data_source_url: "collection://ds-123".to_string(),
            title_property: Some("Topic / Thread Title".to_string()),
            properties,
        }
    }

    fn notion_writer_step(metadata: Value) -> crate::WorkflowPlanStep {
        crate::WorkflowPlanStep {
            step_id: "write_notion_rows".to_string(),
            kind: "deliver".to_string(),
            objective: "Write filtered rows to the Notion database.".to_string(),
            agent_role: "writer".to_string(),
            depends_on: vec!["filter_leads".to_string()],
            input_refs: vec![crate::AutomationFlowInputRef {
                from_step_id: "filter_leads".to_string(),
                alias: "filtered_leads".to_string(),
            }],
            output_contract: Some(crate::AutomationFlowOutputContract {
                kind: "structured_json".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
                enforcement: None,
                schema: Some(json!({"type": "object"})),
                summary_guidance: None,
            }),
            metadata: Some(metadata),
        }
    }

    #[test]
    fn notion_writer_candidate_detection_ignores_global_prompt_intent() {
        let research_step = crate::WorkflowPlanStep {
            step_id: "search_reddit".to_string(),
            kind: "research".to_string(),
            objective: "Search Reddit for infrastructure lead candidates.".to_string(),
            agent_role: "researcher".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            metadata: None,
        };

        assert!(
            !workflow_step_is_notion_database_writer_candidate(&research_step),
            "a non-writer step should not be classified from the workflow-level Notion goal"
        );

        let writer_step = notion_writer_step(json!({
            "connector": "notion",
            "connector_writer": true
        }));
        assert!(workflow_step_is_notion_database_writer_candidate(
            &writer_step
        ));
    }

    #[test]
    fn notion_connector_writer_enrichment_preserves_valid_schema_mappings() {
        let mut step = notion_writer_step(json!({
            "connector": "notion",
            "connector_writer": true,
            "notion_data_source_url": "collection://ds-123",
            "input_alias": "filtered_leads",
            "source_array_key": "leads",
            "title_property": "Topic / Thread Title",
            "property_mappings": {
                "Topic / Thread Title": "topic_thread_title",
                "Thread Link": "thread_link"
            },
            "default_properties": {
                "Status": "New Lead"
            },
            "duplicate_keys": ["Thread Link"]
        }));

        let row = enrich_notion_connector_writer_step_from_schema(&mut step, &test_notion_schema())
            .expect("valid notion writer metadata");

        assert_eq!(row.get("connector").and_then(Value::as_str), Some("notion"));
        let metadata = step.metadata.as_ref().expect("metadata");
        assert_eq!(
            metadata.get("writer_kind").and_then(Value::as_str),
            Some("database_rows")
        );
        assert_eq!(
            metadata
                .get("notion_data_source_id")
                .and_then(Value::as_str),
            Some("ds-123")
        );
        assert_eq!(
            metadata
                .get("duplicate_keys")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(Value::as_str),
            Some("Thread Link")
        );
        assert_eq!(step.input_refs.len(), 1);
    }

    #[test]
    fn notion_connector_writer_rejects_invalid_status_default() {
        let mut step = notion_writer_step(json!({
            "connector": "notion",
            "connector_writer": true,
            "notion_data_source_url": "collection://ds-123",
            "input_alias": "filtered_leads",
            "title_property": "Topic / Thread Title",
            "property_mappings": {
                "Topic / Thread Title": "topic_thread_title",
                "Thread Link": "thread_link"
            },
            "default_properties": {
                "Status": "Not started"
            },
            "duplicate_keys": ["Thread Link"]
        }));

        let error =
            enrich_notion_connector_writer_step_from_schema(&mut step, &test_notion_schema())
                .expect_err("invalid status default should be rejected");

        assert!(error.contains("Status"));
        assert!(error.contains("Not started"));
        assert!(error.contains("New Lead"));
    }

    #[test]
    fn notion_schema_extraction_reads_nested_mcp_payload() {
        let payload = json!({
            "output": {
                "data_source": {
                    "id": "ds-456",
                    "url": "collection://ds-456",
                    "properties": {
                        "Name": {"type": "title"},
                        "Status": {
                            "type": "status",
                            "status": {
                                "options": [
                                    {"name": "New Lead"},
                                    {"name": "Reviewed"}
                                ]
                            }
                        }
                    }
                }
            }
        });

        let schema = extract_notion_data_source_schema(&payload, Some("collection://ds-456"))
            .expect("schema");

        assert_eq!(schema.data_source_id, "ds-456");
        assert_eq!(schema.title_property.as_deref(), Some("Name"));
        assert!(schema.properties.contains_key("Status"));
        assert!(schema.properties["Status"].options.contains("Reviewed"));
    }
}
