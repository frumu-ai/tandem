use super::*;
use std::collections::HashMap;
use tandem_core::{tool_name_matches_profile, tool_schema_matches_profile, ToolCapabilityProfile};
use tandem_types::ToolSchema;

pub(crate) fn automation_expand_effective_offered_tools(
    offered_tools: &[String],
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    let mut effective = Vec::new();
    for offered_tool in offered_tools {
        let offered_tool = offered_tool.trim();
        if offered_tool.is_empty() {
            continue;
        }
        if offered_tool == "*" {
            effective.extend(available_tool_names.iter().cloned());
            continue;
        }
        if let Some(prefix) = offered_tool.strip_suffix('*') {
            effective.extend(
                available_tool_names
                    .iter()
                    .filter(|tool_name| tool_name.starts_with(prefix))
                    .cloned(),
            );
            continue;
        }
        if available_tool_names.contains(offered_tool) {
            effective.push(offered_tool.to_string());
        }
    }
    effective.sort();
    effective.dedup();
    effective
}

pub(crate) fn automation_tool_name_tokens(tool_name: &str) -> Vec<String> {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>()
}

pub(crate) fn automation_tool_name_is_email_delivery(tool_name: &str) -> bool {
    tool_name_matches_profile(tool_name, ToolCapabilityProfile::EmailDelivery)
}

pub(crate) fn automation_discovered_tools_for_predicate<F>(
    tools: impl IntoIterator<Item = String>,
    predicate: F,
) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    let mut discovered = tools
        .into_iter()
        .filter(|tool_name| predicate(tool_name))
        .collect::<Vec<_>>();
    discovered.sort();
    discovered.dedup();
    discovered
}

fn automation_prompt_token_estimate(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    chars.div_ceil(4)
}

fn automation_tool_schema_chars<T: serde::Serialize>(schemas: &[T]) -> u64 {
    schemas
        .iter()
        .map(|schema| {
            serde_json::to_string(schema)
                .map(|text| text.len() as u64)
                .unwrap_or(0)
        })
        .sum()
}

pub(crate) fn automation_tool_capability_ids(
    node: &AutomationFlowNode,
    execution_mode: &str,
) -> Vec<String> {
    let mut capabilities = Vec::new();
    let required_tools = automation_node_required_tools(node);
    let requires_workspace_read =
        !node.input_refs.is_empty() || required_tools.iter().any(|tool| tool == "read");
    let requires_workspace_discover = automation_node_required_output_path(node).is_some()
        || automation_output_validator_kind(node)
            == crate::AutomationOutputValidatorKind::ResearchBrief
        || required_tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list"));
    let requires_artifact_write = automation_node_required_output_path(node).is_some()
        || required_tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "write" | "edit" | "apply_patch"));
    if requires_workspace_read {
        capabilities.push("workspace_read".to_string());
    }
    if requires_workspace_discover {
        capabilities.push("workspace_discover".to_string());
    }
    if requires_artifact_write {
        capabilities.push("artifact_write".to_string());
    }
    if automation_node_web_research_expected(node) {
        capabilities.push("web_research".to_string());
    }
    if automation_node_requires_email_delivery(node) {
        capabilities.push("email_send".to_string());
        capabilities.push("email_draft".to_string());
    }
    if automation_node_is_code_workflow(node)
        && (execution_mode == "git_patch" || execution_mode == "filesystem_patch")
    {
        capabilities.push("verify_command".to_string());
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

pub(crate) fn automation_tool_name_is_email_send(tool_name: &str) -> bool {
    tool_name_matches_profile(tool_name, ToolCapabilityProfile::EmailSend)
}

pub(crate) fn automation_tool_name_is_email_draft(tool_name: &str) -> bool {
    tool_name_matches_profile(tool_name, ToolCapabilityProfile::EmailDraft)
}

fn automation_capability_profile(capability_id: &str) -> Option<ToolCapabilityProfile> {
    match capability_id {
        "workspace_read" => Some(ToolCapabilityProfile::WorkspaceRead),
        "workspace_discover" => Some(ToolCapabilityProfile::WorkspaceDiscover),
        "artifact_write" => Some(ToolCapabilityProfile::ArtifactWrite),
        "web_research" => Some(ToolCapabilityProfile::WebResearch),
        "email_send" => Some(ToolCapabilityProfile::EmailSend),
        "email_draft" => Some(ToolCapabilityProfile::EmailDraft),
        "email_delivery" => Some(ToolCapabilityProfile::EmailDelivery),
        "verify_command" => Some(ToolCapabilityProfile::VerifyCommand),
        _ => None,
    }
}

fn automation_capability_matches_tool_name(capability_id: &str, tool_name: &str) -> bool {
    automation_capability_profile(capability_id)
        .is_some_and(|profile| tool_name_matches_profile(tool_name, profile))
}

pub(crate) fn automation_capability_matches_tool(capability_id: &str, tool_name: &str) -> bool {
    automation_capability_matches_tool_name(capability_id, tool_name)
}

fn automation_capability_matches_schema(capability_id: &str, schema: &ToolSchema) -> bool {
    automation_capability_profile(capability_id)
        .is_some_and(|profile| tool_schema_matches_profile(schema, profile))
}

fn automation_matching_tool_names(
    tools: impl IntoIterator<Item = String>,
    available_tool_schemas_by_name: &HashMap<String, &ToolSchema>,
    capability_id: &str,
) -> Vec<String> {
    automation_discovered_tools_for_predicate(tools, |tool_name| {
        available_tool_schemas_by_name
            .get(tool_name)
            .map(|schema| automation_capability_matches_schema(capability_id, schema))
            .unwrap_or_else(|| automation_capability_matches_tool_name(capability_id, tool_name))
    })
}

pub(crate) fn automation_resolve_capabilities(
    node: &AutomationFlowNode,
    execution_mode: &str,
    offered_tools: &[String],
    available_tool_names: &HashSet<String>,
) -> Value {
    automation_resolve_capabilities_with_schemas(
        node,
        execution_mode,
        offered_tools,
        available_tool_names,
        &[],
    )
}

pub(crate) fn automation_resolve_capabilities_with_schemas(
    node: &AutomationFlowNode,
    execution_mode: &str,
    offered_tools: &[String],
    available_tool_names: &HashSet<String>,
    available_tool_schemas: &[ToolSchema],
) -> Value {
    let effective_offered_tools =
        automation_expand_effective_offered_tools(offered_tools, available_tool_names);
    let required_capabilities = automation_tool_capability_ids(node, execution_mode);
    let available_tool_schemas_by_name = available_tool_schemas
        .iter()
        .map(|schema| (schema.name.clone(), schema))
        .collect::<HashMap<_, _>>();
    let mut resolved = serde_json::Map::new();
    let mut missing = Vec::new();
    for capability_id in &required_capabilities {
        let available_matches = automation_matching_tool_names(
            available_tool_names.iter().cloned(),
            &available_tool_schemas_by_name,
            capability_id,
        );
        let offered_matches = automation_matching_tool_names(
            effective_offered_tools.clone(),
            &available_tool_schemas_by_name,
            capability_id,
        );
        let status = if !offered_matches.is_empty() {
            "resolved"
        } else if !available_matches.is_empty() {
            "not_offered"
        } else {
            "unavailable"
        };
        if status != "resolved" {
            missing.push(capability_id.clone());
        }
        resolved.insert(
            capability_id.clone(),
            json!({
                "status": status,
                "offered_tools": offered_matches,
                "available_tools": available_matches,
            }),
        );
    }
    let mut output = serde_json::Map::new();
    output.insert(
        "required_capabilities".to_string(),
        json!(required_capabilities),
    );
    output.insert("resolved".to_string(), json!(resolved));
    output.insert("missing_capabilities".to_string(), json!(missing));
    if automation_node_requires_email_delivery(node) {
        let available_email_like_tools = automation_matching_tool_names(
            available_tool_names.iter().cloned(),
            &available_tool_schemas_by_name,
            "email_delivery",
        );
        let offered_email_like_tools = automation_matching_tool_names(
            effective_offered_tools.clone(),
            &available_tool_schemas_by_name,
            "email_delivery",
        );
        let available_send_tools = automation_matching_tool_names(
            available_tool_names.iter().cloned(),
            &available_tool_schemas_by_name,
            "email_send",
        );
        let offered_send_tools = automation_matching_tool_names(
            effective_offered_tools.clone(),
            &available_tool_schemas_by_name,
            "email_send",
        );
        let available_draft_tools = automation_matching_tool_names(
            available_tool_names.iter().cloned(),
            &available_tool_schemas_by_name,
            "email_draft",
        );
        let offered_draft_tools = automation_matching_tool_names(
            effective_offered_tools.clone(),
            &available_tool_schemas_by_name,
            "email_draft",
        );
        output.insert(
            "email_tool_diagnostics".to_string(),
            json!({
                "available_tools": available_email_like_tools,
                "offered_tools": offered_email_like_tools,
                "available_send_tools": available_send_tools,
                "offered_send_tools": offered_send_tools,
                "available_draft_tools": available_draft_tools,
                "offered_draft_tools": offered_draft_tools,
            }),
        );
    }
    Value::Object(output)
}

pub(crate) fn build_automation_prompt_preflight<T: serde::Serialize>(
    prompt: &str,
    offered_tools: &[String],
    offered_tool_schemas: &[T],
    execution_mode: &str,
    capability_resolution: &Value,
    prompt_compaction: &str,
    degraded_prompt: bool,
) -> Value {
    let estimated_prompt_tokens = automation_prompt_token_estimate(prompt);
    let offered_tool_schema_chars = automation_tool_schema_chars(offered_tool_schemas);
    let budget_status = if estimated_prompt_tokens >= AUTOMATION_PROMPT_HIGH_TOKENS
        || offered_tool_schema_chars >= AUTOMATION_TOOL_SCHEMA_HIGH_CHARS
    {
        "high"
    } else if estimated_prompt_tokens >= AUTOMATION_PROMPT_WARNING_TOKENS
        || offered_tool_schema_chars >= AUTOMATION_TOOL_SCHEMA_WARNING_CHARS
    {
        "warning"
    } else {
        "ok"
    };
    json!({
        "rendered_prompt_chars": prompt.chars().count(),
        "estimated_prompt_tokens": estimated_prompt_tokens,
        "offered_tools": offered_tools,
        "offered_tool_schema_count": offered_tool_schemas.len(),
        "offered_tool_schema_chars": offered_tool_schema_chars,
        "execution_mode": execution_mode,
        "budget_status": budget_status,
        "degraded_prompt": degraded_prompt,
        "prompt_compaction": prompt_compaction,
        "required_capability_availability": capability_resolution,
    })
}

pub(crate) fn automation_preflight_should_degrade(preflight: &Value) -> bool {
    preflight
        .get("budget_status")
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "warning" | "high"))
}

pub(crate) fn summarize_json_keys(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            json!({
                "type": "object",
                "keys": keys,
                "field_count": map.len()
            })
        }
        Value::Array(rows) => json!({
            "type": "array",
            "length": rows.len()
        }),
        Value::String(text) => json!({
            "type": "string",
            "length": text.len()
        }),
        Value::Null => json!({"type": "null"}),
        Value::Bool(_) => json!({"type": "boolean"}),
        Value::Number(_) => json!({"type": "number"}),
    }
}

fn automation_attempt_evidence_from_tool_telemetry<'a>(
    tool_telemetry: &'a Value,
) -> Option<&'a Value> {
    tool_telemetry.get("attempt_evidence")
}

fn automation_attempt_evidence_read_paths(tool_telemetry: &Value) -> Vec<String> {
    automation_attempt_evidence_from_tool_telemetry(tool_telemetry)
        .and_then(|value| value.get("evidence"))
        .and_then(|value| value.get("read_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn automation_attempt_evidence_web_research_status(tool_telemetry: &Value) -> Option<String> {
    automation_attempt_evidence_from_tool_telemetry(tool_telemetry)
        .and_then(|value| value.get("evidence"))
        .and_then(|value| value.get("web_research"))
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn automation_attempt_evidence_delivery_status(tool_telemetry: &Value) -> Option<String> {
    automation_attempt_evidence_from_tool_telemetry(tool_telemetry)
        .and_then(|value| value.get("delivery"))
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn automation_attempt_evidence_missing_capabilities(tool_telemetry: &Value) -> Vec<String> {
    automation_attempt_evidence_from_tool_telemetry(tool_telemetry)
        .and_then(|value| value.get("capability_resolution"))
        .and_then(|value| value.get("missing_capabilities"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn automation_capability_resolution_email_tools(tool_telemetry: &Value, key: &str) -> Vec<String> {
    tool_telemetry
        .get("capability_resolution")
        .and_then(|value| value.get("email_tool_diagnostics"))
        .and_then(|value| value.get(key))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn automation_capability_resolution_mcp_tools(tool_telemetry: &Value, key: &str) -> Vec<String> {
    tool_telemetry
        .get("capability_resolution")
        .and_then(|value| value.get("mcp_tool_diagnostics"))
        .and_then(|value| value.get(key))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn automation_normalize_server_list(raw: &[String]) -> Vec<String> {
    let mut servers = raw
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    servers.sort();
    servers.dedup();
    servers
}

fn automation_tool_names_for_mcp_server(tool_names: &[String], server_name: &str) -> Vec<String> {
    let prefix = format!(
        "mcp.{}.",
        crate::http::mcp::mcp_namespace_segment(server_name)
    );
    let mut tools = tool_names
        .iter()
        .filter(|tool_name| tool_name.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    tools.sort();
    tools.dedup();
    tools
}

fn automation_merge_mcp_capability_diagnostics(
    capability_resolution: &mut Value,
    mcp_diagnostics: &Value,
) {
    let Some(root) = capability_resolution.as_object_mut() else {
        return;
    };
    root.insert("mcp_tool_diagnostics".to_string(), mcp_diagnostics.clone());
    if let Some(email_diagnostics) = root
        .get_mut("email_tool_diagnostics")
        .and_then(Value::as_object_mut)
    {
        for key in [
            "selected_servers",
            "remote_tools",
            "registered_tools",
            "remote_email_like_tools",
            "registered_email_like_tools",
            "servers",
        ] {
            if let Some(value) = mcp_diagnostics.get(key).cloned() {
                email_diagnostics.insert(key.to_string(), value);
            }
        }
    }
}

fn automation_selected_mcp_servers_from_allowlist(
    allowlist: &[String],
    known_server_names: &[String],
) -> Vec<String> {
    let mut selected = Vec::new();
    for server_name in known_server_names {
        let namespace = crate::http::mcp::mcp_namespace_segment(server_name);
        if allowlist.iter().any(|entry| {
            let normalized = entry.trim();
            normalized == format!("mcp.{namespace}.*")
                || normalized.starts_with(&format!("mcp.{namespace}."))
        }) {
            selected.push(server_name.clone());
        }
    }
    selected.sort();
    selected.dedup();
    selected
}

fn infer_selected_mcp_servers_from_capability_state(
    allowed_servers: &[String],
    allowlist: &[String],
    enabled_server_names: &[String],
    requires_email_delivery: bool,
) -> Vec<String> {
    let mut selected_servers = automation_normalize_server_list(allowed_servers);
    selected_servers.extend(automation_selected_mcp_servers_from_allowlist(
        allowlist,
        enabled_server_names,
    ));
    selected_servers.sort();
    selected_servers.dedup();
    if !selected_servers.is_empty() {
        return selected_servers;
    }
    let wildcard_allowed = allowlist.iter().any(|entry| entry.trim() == "*");
    if wildcard_allowed || requires_email_delivery {
        return enabled_server_names.to_vec();
    }
    Vec::new()
}

async fn sync_automation_allowed_mcp_servers(
    state: &AppState,
    node: &AutomationFlowNode,
    allowed_servers: &[String],
    allowlist: &[String],
) -> Value {
    let mcp_servers = state.mcp.list().await;
    let enabled_server_names = mcp_servers
        .values()
        .filter(|server| server.enabled)
        .map(|server| server.name.clone())
        .collect::<Vec<_>>();
    let selected_servers = infer_selected_mcp_servers_from_capability_state(
        allowed_servers,
        allowlist,
        &enabled_server_names,
        automation_node_requires_email_delivery(node),
    );
    if selected_servers.is_empty() {
        return json!({
            "selected_servers": [],
            "servers": [],
            "remote_tools": [],
            "registered_tools": [],
            "remote_email_like_tools": [],
            "registered_email_like_tools": [],
        });
    }
    let mut server_rows = Vec::new();
    for server_name in &selected_servers {
        let server_record = mcp_servers.get(server_name);
        let exists = server_record.is_some();
        let enabled = server_record.is_some_and(|server| server.enabled);
        let connected = if enabled {
            state.mcp.connect(server_name).await
        } else {
            false
        };
        let sync_count = if connected {
            crate::http::mcp::sync_mcp_tools_for_server(state, server_name).await as u64
        } else {
            0
        };
        let sync_error = if !exists {
            Some("server_not_found")
        } else if !enabled {
            Some("server_disabled")
        } else if !connected {
            Some("connect_failed")
        } else {
            None
        };
        server_rows.push(json!({
            "name": server_name,
            "exists": exists,
            "enabled": enabled,
            "connected": connected,
            "sync_error": sync_error,
            "registered_tool_count_after_sync": sync_count,
        }));
    }

    let remote_tools = state.mcp.list_tools().await;
    let registered_tool_names = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<Vec<_>>();

    let mut all_remote_names = Vec::new();
    let mut all_registered_names = Vec::new();
    let mut all_remote_email_like_names = Vec::new();
    let mut all_registered_email_like_names = Vec::new();

    for row in &mut server_rows {
        let Some(server_name) = row.get("name").and_then(Value::as_str) else {
            continue;
        };
        let mut remote_names = remote_tools
            .iter()
            .filter(|tool| tool.server_name == server_name)
            .map(|tool| {
                if tool.namespaced_name.trim().is_empty() {
                    format!(
                        "mcp.{}.{}",
                        crate::http::mcp::mcp_namespace_segment(server_name),
                        tool.tool_name
                    )
                } else {
                    tool.namespaced_name.clone()
                }
            })
            .collect::<Vec<_>>();
        remote_names.sort();
        remote_names.dedup();

        let registered_names =
            automation_tool_names_for_mcp_server(&registered_tool_names, server_name);
        let remote_email_like_names = automation_discovered_tools_for_predicate(
            remote_names.clone(),
            automation_tool_name_is_email_delivery,
        );
        let registered_email_like_names = automation_discovered_tools_for_predicate(
            registered_names.clone(),
            automation_tool_name_is_email_delivery,
        );

        all_remote_names.extend(remote_names.clone());
        all_registered_names.extend(registered_names.clone());
        all_remote_email_like_names.extend(remote_email_like_names.clone());
        all_registered_email_like_names.extend(registered_email_like_names.clone());

        if let Some(object) = row.as_object_mut() {
            object.insert("remote_tools".to_string(), json!(remote_names));
            object.insert("registered_tools".to_string(), json!(registered_names));
            object.insert(
                "remote_email_like_tools".to_string(),
                json!(remote_email_like_names),
            );
            object.insert(
                "registered_email_like_tools".to_string(),
                json!(registered_email_like_names),
            );
        }
    }

    all_remote_names.sort();
    all_remote_names.dedup();
    all_registered_names.sort();
    all_registered_names.dedup();
    all_remote_email_like_names.sort();
    all_remote_email_like_names.dedup();
    all_registered_email_like_names.sort();
    all_registered_email_like_names.dedup();

    json!({
        "selected_servers": selected_servers,
        "servers": server_rows,
        "remote_tools": all_remote_names,
        "registered_tools": all_registered_names,
        "remote_email_like_tools": all_remote_email_like_names,
        "registered_email_like_tools": all_registered_email_like_names,
    })
}
