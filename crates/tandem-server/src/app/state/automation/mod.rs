use std::collections::HashSet;
use std::path::PathBuf;

pub(crate) mod assessment;
pub(crate) mod enforcement;
pub(crate) mod extraction;
pub(crate) mod lifecycle;
pub(crate) mod node_output;
pub(crate) mod path_hygiene;
pub(crate) mod rate_limit;
pub(crate) mod scheduler;
pub(crate) mod types;
pub(crate) mod upstream;
pub(crate) mod validation;
pub(crate) mod verification;
use assessment::*;
use enforcement::*;
use extraction::*;
use lifecycle::*;
pub use lifecycle::{record_automation_lifecycle_event, record_automation_workflow_state_events};
pub(crate) use node_output::enrich_automation_node_output_for_contract;
pub(crate) use node_output::research_required_next_tool_actions;
use node_output::*;
use path_hygiene::*;
pub use scheduler::{
    AutomationScheduler, PreexistingArtifactRegistry, QueueReason, SchedulerMetadata,
    ValidatedArtifact,
};
use types::*;
use upstream::*;
use validation::*;
use verification::*;

pub fn automation_node_output_enforcement(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputEnforcement {
    enforcement::automation_node_output_enforcement(node)
}

use serde_json::{json, Value};
use tandem_plan_compiler::api as compiler_api;
use tandem_types::{
    MessagePart, MessagePartInput, MessageRole, ModelSpec, PrewriteCoverageMode,
    PrewriteRequirements, SendMessageRequest, Session, ToolMode,
};

use super::*;
use crate::capability_resolver::{self};
use crate::config::{self};
use crate::util::time::now_ms;
pub async fn run_automation_v2_scheduler(state: AppState) {
    crate::app::tasks::run_automation_v2_scheduler(state).await
}

pub(crate) fn is_automation_approval_node(node: &AutomationFlowNode) -> bool {
    matches!(node.stage_kind, Some(AutomationNodeStageKind::Approval))
        || node
            .gate
            .as_ref()
            .map(|gate| gate.required)
            .unwrap_or(false)
}

pub(crate) fn automation_guardrail_failure(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<String> {
    if let Some(max_runtime_ms) = automation.execution.max_total_runtime_ms {
        if let Some(started_at_ms) = run.started_at_ms {
            let elapsed = now_ms().saturating_sub(started_at_ms);
            if elapsed >= max_runtime_ms {
                return Some(format!(
                    "run exceeded max_total_runtime_ms ({elapsed}/{max_runtime_ms})"
                ));
            }
        }
    }
    if let Some(max_total_tokens) = automation.execution.max_total_tokens {
        if run.total_tokens >= max_total_tokens {
            return Some(format!(
                "run exceeded max_total_tokens ({}/{})",
                run.total_tokens, max_total_tokens
            ));
        }
    }
    if let Some(max_total_cost_usd) = automation.execution.max_total_cost_usd {
        if run.estimated_cost_usd >= max_total_cost_usd {
            return Some(format!(
                "run exceeded max_total_cost_usd ({:.4}/{:.4})",
                run.estimated_cost_usd, max_total_cost_usd
            ));
        }
    }
    None
}

const AUTOMATION_PROMPT_WARNING_TOKENS: u64 = 2_400;
const AUTOMATION_PROMPT_HIGH_TOKENS: u64 = 3_200;
const AUTOMATION_TOOL_SCHEMA_WARNING_CHARS: u64 = 18_000;
const AUTOMATION_TOOL_SCHEMA_HIGH_CHARS: u64 = 26_000;

#[derive(Clone, Copy, Debug, Default)]
struct AutomationPromptRenderOptions {
    summary_only_upstream: bool,
}

fn automation_tool_name_is_workspace_read(tool_name: &str) -> bool {
    tool_name.trim().eq_ignore_ascii_case("read")
}

fn automation_tool_name_is_workspace_discover(tool_name: &str) -> bool {
    matches!(
        tool_name
            .trim()
            .to_ascii_lowercase()
            .replace('-', "_")
            .as_str(),
        "glob" | "search" | "grep" | "codesearch" | "ls" | "list"
    )
}

fn automation_tool_name_is_artifact_write(tool_name: &str) -> bool {
    matches!(
        tool_name
            .trim()
            .to_ascii_lowercase()
            .replace('-', "_")
            .as_str(),
        "write" | "edit" | "apply_patch"
    )
}

fn automation_tool_name_is_web_research(tool_name: &str) -> bool {
    matches!(
        tool_name
            .trim()
            .to_ascii_lowercase()
            .replace('-', "_")
            .as_str(),
        "websearch" | "webfetch" | "webfetch_html"
    )
}

fn automation_tool_name_is_verify_command(tool_name: &str) -> bool {
    tool_name.trim().eq_ignore_ascii_case("bash")
}

fn automation_tool_name_tokens(tool_name: &str) -> Vec<String> {
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

fn automation_tool_name_contains_token(tokens: &[String], needle: &str) -> bool {
    tokens.iter().any(|token| token == needle)
}

fn automation_tool_name_compact(tool_name: &str) -> String {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
}

fn automation_tool_name_is_email_send(tool_name: &str) -> bool {
    let tokens = automation_tool_name_tokens(tool_name);
    let compact = automation_tool_name_compact(tool_name);
    automation_tool_name_is_email_delivery(tool_name)
        && (automation_tool_name_contains_token(&tokens, "send")
            || automation_tool_name_contains_token(&tokens, "deliver")
            || automation_tool_name_contains_token(&tokens, "reply")
            || compact.contains("sendemail")
            || compact.contains("emailsend")
            || compact.contains("replyemail")
            || compact.contains("emailreply"))
}

fn automation_tool_name_is_email_draft(tool_name: &str) -> bool {
    let tokens = automation_tool_name_tokens(tool_name);
    let compact = automation_tool_name_compact(tool_name);
    automation_tool_name_is_email_delivery(tool_name)
        && (automation_tool_name_contains_token(&tokens, "draft")
            || automation_tool_name_contains_token(&tokens, "compose")
            || compact.contains("draftemail")
            || compact.contains("emaildraft")
            || compact.contains("composeemail")
            || compact.contains("emailcompose"))
}

fn automation_expand_effective_offered_tools(
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

fn automation_discovered_tools_for_predicate<F>(
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

pub(crate) fn automation_step_cost_provenance(
    step_id: &str,
    model_id: Option<String>,
    tokens_in: u64,
    tokens_out: u64,
    computed_cost_usd: f64,
    cumulative_run_cost_usd_at_step_end: f64,
    budget_limit_reached: bool,
) -> Value {
    json!({
        "step_id": step_id,
        "model_id": model_id,
        "tokens_in": tokens_in,
        "tokens_out": tokens_out,
        "computed_cost_usd": computed_cost_usd,
        "cumulative_run_cost_usd_at_step_end": cumulative_run_cost_usd_at_step_end,
        "budget_limit_reached": budget_limit_reached,
    })
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

fn automation_tool_capability_ids(node: &AutomationFlowNode, execution_mode: &str) -> Vec<String> {
    let mut capabilities = Vec::new();
    if !node.input_refs.is_empty()
        || automation_node_required_tools(node)
            .iter()
            .any(|tool| tool == "read")
    {
        capabilities.push("workspace_read".to_string());
    }
    if automation_node_required_output_path(node).is_some()
        || automation_output_validator_kind(node)
            == crate::AutomationOutputValidatorKind::ResearchBrief
    {
        capabilities.push("workspace_discover".to_string());
    }
    if automation_node_required_output_path(node).is_some() {
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

fn automation_capability_matches_tool(capability_id: &str, tool_name: &str) -> bool {
    match capability_id {
        "workspace_read" => automation_tool_name_is_workspace_read(tool_name),
        "workspace_discover" => automation_tool_name_is_workspace_discover(tool_name),
        "artifact_write" => automation_tool_name_is_artifact_write(tool_name),
        "web_research" => automation_tool_name_is_web_research(tool_name),
        "email_send" => automation_tool_name_is_email_send(tool_name),
        "email_draft" => automation_tool_name_is_email_draft(tool_name),
        "verify_command" => automation_tool_name_is_verify_command(tool_name),
        _ => false,
    }
}

fn automation_resolve_capabilities(
    node: &AutomationFlowNode,
    execution_mode: &str,
    offered_tools: &[String],
    available_tool_names: &HashSet<String>,
) -> Value {
    let effective_offered_tools =
        automation_expand_effective_offered_tools(offered_tools, available_tool_names);
    let required_capabilities = automation_tool_capability_ids(node, execution_mode);
    let mut resolved = serde_json::Map::new();
    let mut missing = Vec::new();
    for capability_id in &required_capabilities {
        let available_matches = available_tool_names
            .iter()
            .filter(|tool_name| automation_capability_matches_tool(capability_id, tool_name))
            .cloned()
            .collect::<Vec<_>>();
        let offered_matches = effective_offered_tools
            .iter()
            .filter(|tool_name| automation_capability_matches_tool(capability_id, tool_name))
            .cloned()
            .collect::<Vec<_>>();
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
        let available_email_like_tools = automation_discovered_tools_for_predicate(
            available_tool_names.iter().cloned().collect::<Vec<_>>(),
            automation_tool_name_is_email_delivery,
        );
        let offered_email_like_tools = automation_discovered_tools_for_predicate(
            effective_offered_tools.clone(),
            automation_tool_name_is_email_delivery,
        );
        let available_send_tools = automation_discovered_tools_for_predicate(
            available_tool_names.iter().cloned().collect::<Vec<_>>(),
            automation_tool_name_is_email_send,
        );
        let offered_send_tools = automation_discovered_tools_for_predicate(
            effective_offered_tools.clone(),
            automation_tool_name_is_email_send,
        );
        let available_draft_tools = automation_discovered_tools_for_predicate(
            available_tool_names.iter().cloned().collect::<Vec<_>>(),
            automation_tool_name_is_email_draft,
        );
        let offered_draft_tools = automation_discovered_tools_for_predicate(
            effective_offered_tools.clone(),
            automation_tool_name_is_email_draft,
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

fn automation_preflight_should_degrade(preflight: &Value) -> bool {
    preflight
        .get("budget_status")
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "warning" | "high"))
}

fn summarize_json_keys(value: &Value) -> Value {
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

pub(crate) fn automation_infer_selected_mcp_servers(
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
    let selected_servers = automation_infer_selected_mcp_servers(
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

fn automation_node_delivery_method_value(node: &AutomationFlowNode) -> String {
    automation_node_delivery_method(node).unwrap_or_else(|| "none".to_string())
}

pub(crate) fn automation_output_session_id(output: &Value) -> Option<String> {
    output
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| {
            content
                .get("session_id")
                .or_else(|| content.get("sessionId"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn build_automation_pending_gate(
    node: &AutomationFlowNode,
) -> Option<AutomationPendingGate> {
    let gate = node.gate.as_ref()?;
    Some(AutomationPendingGate {
        node_id: node.node_id.clone(),
        title: node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("title"))
            .and_then(Value::as_str)
            .unwrap_or(node.objective.as_str())
            .to_string(),
        instructions: gate.instructions.clone(),
        decisions: gate.decisions.clone(),
        rework_targets: gate.rework_targets.clone(),
        requested_at_ms: now_ms(),
        upstream_node_ids: node.depends_on.clone(),
    })
}

fn automation_node_builder_metadata(node: &AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn automation_node_research_stage(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "research_stage")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn automation_node_is_research_finalize(node: &AutomationFlowNode) -> bool {
    automation_node_research_stage(node).as_deref() == Some("research_finalize")
}

fn automation_node_uses_upstream_validation_evidence(node: &AutomationFlowNode) -> bool {
    if automation_node_is_research_finalize(node) {
        return true;
    }
    let has_upstream_inputs = !node.input_refs.is_empty() || !node.depends_on.is_empty();
    if !has_upstream_inputs {
        return false;
    }
    if automation_node_requires_email_delivery(node) {
        return true;
    }
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        contract_kind.as_str(),
        "brief" | "report_markdown" | "text_summary" | "review_summary" | "approval_gate"
    )
}

fn automation_node_preserves_full_upstream_inputs(node: &AutomationFlowNode) -> bool {
    if !automation_node_uses_upstream_validation_evidence(node) {
        return false;
    }
    matches!(
        node.output_contract
            .as_ref()
            .map(|contract| contract.kind.trim().to_ascii_lowercase())
            .as_deref(),
        Some("report_markdown" | "text_summary")
    )
}

fn automation_node_builder_priority(node: &AutomationFlowNode) -> i32 {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("priority"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0)
}

fn truncate_path_list_for_prompt(paths: Vec<String>, limit: usize) -> Vec<String> {
    let mut deduped = normalize_non_empty_list(paths);
    if deduped.len() > limit {
        deduped.truncate(limit);
    }
    deduped
}

fn value_object_path_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

fn render_research_finalize_upstream_summary(upstream_inputs: &[Value]) -> Option<String> {
    let source_inventory =
        automation_upstream_output_for_alias(upstream_inputs, "source_inventory")
            .and_then(automation_upstream_structured_handoff);
    let local_source_notes =
        automation_upstream_output_for_alias(upstream_inputs, "local_source_notes")
            .and_then(automation_upstream_structured_handoff);
    let external_research =
        automation_upstream_output_for_alias(upstream_inputs, "external_research")
            .and_then(automation_upstream_structured_handoff);

    let discovered_files = source_inventory
        .and_then(|handoff| handoff.get("discovered_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let priority_files = source_inventory
        .and_then(|handoff| handoff.get("priority_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let files_reviewed = local_source_notes
        .and_then(|handoff| handoff.get("files_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let files_not_reviewed = local_source_notes
        .and_then(|handoff| handoff.get("files_not_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let web_sources_reviewed = external_research
        .and_then(|handoff| handoff.get("sources_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "url")
                        .or_else(|| value_object_path_field(row, "path")),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let discovered_files = truncate_path_list_for_prompt(discovered_files, 12);
    let priority_files = truncate_path_list_for_prompt(priority_files, 12);
    let files_reviewed = truncate_path_list_for_prompt(files_reviewed, 12);
    let files_not_reviewed = truncate_path_list_for_prompt(files_not_reviewed, 12);
    let web_sources_reviewed = truncate_path_list_for_prompt(web_sources_reviewed, 8);

    if discovered_files.is_empty()
        && priority_files.is_empty()
        && files_reviewed.is_empty()
        && files_not_reviewed.is_empty()
        && web_sources_reviewed.is_empty()
    {
        return None;
    }

    let list_or_none = |items: &[String]| {
        if items.is_empty() {
            "none recorded".to_string()
        } else {
            items
                .iter()
                .map(|item| format!("- `{}`", item))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    Some(format!(
        "Research Coverage Summary:\nRelevant discovered files from upstream:\n{}\nPriority paths from upstream:\n{}\nUpstream files already reviewed:\n{}\nUpstream files already marked not reviewed:\n{}\nUpstream web sources reviewed:\n{}\nFinal brief rule: every relevant discovered file should appear in `Files reviewed` or `Files not reviewed`, and proof points must stay citation-backed.",
        list_or_none(&discovered_files),
        list_or_none(&priority_files),
        list_or_none(&files_reviewed),
        list_or_none(&files_not_reviewed),
        list_or_none(&web_sources_reviewed),
    ))
}

fn render_upstream_synthesis_guidance(
    node: &AutomationFlowNode,
    upstream_inputs: &[Value],
) -> Option<String> {
    if upstream_inputs.is_empty() || !automation_node_uses_upstream_validation_evidence(node) {
        return None;
    }
    let artifact_paths = upstream_inputs
        .iter()
        .filter_map(|input| input.get("output"))
        .filter_map(|output| {
            output
                .pointer("/content/path")
                .or_else(|| output.pointer("/content/data/path"))
                .or_else(|| output.pointer("/path"))
        })
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let artifact_paths = truncate_path_list_for_prompt(artifact_paths, 8);
    let artifact_paths_summary = if artifact_paths.is_empty() {
        "none listed".to_string()
    } else {
        artifact_paths
            .iter()
            .map(|path| format!("- `{}`", path))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut lines = vec![
        "Upstream synthesis rules:".to_string(),
        "- Treat the upstream inputs as the full source of truth for this step.".to_string(),
        "- Carry forward the concrete terminology, proof points, objections, risks, and citations already present upstream; do not collapse them into a vague generic recap.".to_string(),
        "- If an upstream input includes a concrete artifact path, read that artifact before finalizing whenever you need the full body, exact wording, or strongest evidence.".to_string(),
        format!("Upstream artifact paths:\n{}", artifact_paths_summary),
    ];
    if automation_node_requires_email_delivery(node) {
        lines.push("- For email delivery, use the compiled upstream report/body as the email body source of truth. Convert format faithfully if needed, but do not replace it with a shorter teaser or improvised summary.".to_string());
        lines.push("- If multiple upstream artifacts exist, prefer the final report artifact over intermediate notes unless the objective explicitly says otherwise.".to_string());
    }
    if matches!(
        node.output_contract
            .as_ref()
            .map(|contract| contract.kind.trim().to_ascii_lowercase())
            .as_deref(),
        Some("report_markdown" | "text_summary")
    ) {
        lines.push("- For final report synthesis, preserve the upstream terminology, named entities, metrics, objections, and proof points; do not collapse them into a generic strategic summary.".to_string());
        lines.push("- When the upstream evidence is rich, the final report should read like a synthesis of those artifacts, not a high-level position statement detached from them.".to_string());
    }
    Some(lines.join("\n"))
}

fn automation_prompt_html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn automation_prompt_render_canonical_html_body(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "<p></p>".to_string();
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.contains("<html") || lowered.contains("<body") || lowered.contains("<div") {
        return trimmed.to_string();
    }

    let mut html = String::new();
    let mut in_list = false;
    let flush_list = |html: &mut String, in_list: &mut bool| {
        if *in_list {
            html.push_str("</ul>");
            *in_list = false;
        }
    };

    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush_list(&mut html, &mut in_list);
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h3>{}</h3>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h2>{}</h2>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h1>{}</h1>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            html.push_str(&format!(
                "<li>{}</li>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        flush_list(&mut html, &mut in_list);
        html.push_str(&format!("<p>{}</p>", automation_prompt_html_escape(line)));
    }
    flush_list(&mut html, &mut in_list);
    if html.is_empty() {
        "<p></p>".to_string()
    } else {
        html
    }
}

fn render_deterministic_delivery_body(upstream_inputs: &[Value]) -> Option<String> {
    let mut best = upstream_inputs
        .iter()
        .filter_map(|input| {
            let text = input
                .get("output")
                .and_then(|output| output.get("content"))
                .and_then(|content| content.get("text"))
                .or_else(|| input.get("text"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)?;
            let path = input
                .get("output")
                .and_then(|output| output.get("content"))
                .and_then(|content| content.get("path"))
                .or_else(|| input.get("path"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "upstream artifact".to_string());
            Some((text, path))
        })
        .collect::<Vec<_>>();
    best.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.1.cmp(&right.1)));
    let (text, source_path) = best.into_iter().next()?;
    let rendered_html = automation_prompt_render_canonical_html_body(&text);
    Some(format!(
        "Deterministic Delivery Body:\n- Source artifact: `{}`\n- Canonical HTML body:\n{}\n- Use this exact body as the delivery source of truth.\n- Do not rewrite the body into a shorter teaser or substitute a fresh summary.",
        source_path,
        rendered_html
            .lines()
            .map(|line| format!("  {}", line))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn split_research_template_config(template_id: &str) -> Option<SplitResearchTemplateConfig> {
    match template_id {
        "marketing-content-pipeline" => Some(SplitResearchTemplateConfig {
            template_id: "marketing-content-pipeline",
            final_node_id: "research-brief",
            final_agent_id: "research",
            discover_node_id: "research-discover-sources",
            discover_agent_id: "research-discover",
            discover_title: "Discover Sources",
            discover_objective: "Enumerate the workspace, identify the relevant source corpus, and prioritize which local files must be read for the marketing brief.",
            discover_display_name: "Research Discover",
            local_node_id: "research-local-sources",
            local_agent_id: "research-local-sources",
            local_title: "Read Local Sources",
            local_objective: "Read the prioritized local product and marketing files and produce source-backed notes for the brief.",
            local_display_name: "Research Local Sources",
            external_node_id: "research-external-research",
            external_agent_id: "research-external",
            external_title: "External Research",
            external_objective: "Perform targeted external research that complements the local source notes and record what web evidence was gathered or unavailable.",
            external_display_name: "Research External",
            final_title: "Research Brief",
            final_objective: "Write `marketing-brief.md` from the structured discovery, local source notes, and external research gathered earlier in the workflow.",
        }),
        "competitor-research-pipeline" => Some(SplitResearchTemplateConfig {
            template_id: "competitor-research-pipeline",
            final_node_id: "scan-market",
            final_agent_id: "market-scan",
            discover_node_id: "scan-market-discover",
            discover_agent_id: "market-discover",
            discover_title: "Discover Market Sources",
            discover_objective: "Identify the local source corpus and file inventory that should guide the competitor scan.",
            discover_display_name: "Market Discover",
            local_node_id: "scan-market-local-sources",
            local_agent_id: "market-local-sources",
            local_title: "Read Market Sources",
            local_objective: "Read the prioritized local competitor and strategy sources before external scanning.",
            local_display_name: "Market Local Sources",
            external_node_id: "scan-market-external-research",
            external_agent_id: "market-external",
            external_title: "Research Market",
            external_objective: "Gather current external competitor evidence guided by the local market context.",
            external_display_name: "Market External",
            final_title: "Scan Market",
            final_objective: "Synthesize the discovered local and external evidence into the final competitor scan.",
        }),
        "weekly-newsletter-builder" => Some(SplitResearchTemplateConfig {
            template_id: "weekly-newsletter-builder",
            final_node_id: "curate-issue",
            final_agent_id: "curator",
            discover_node_id: "curate-issue-discover",
            discover_agent_id: "curator-discover",
            discover_title: "Discover Issue Sources",
            discover_objective: "Identify the local source corpus and candidate files that should feed this week's issue.",
            discover_display_name: "Curator Discover",
            local_node_id: "curate-issue-local-sources",
            local_agent_id: "curator-local-sources",
            local_title: "Read Issue Sources",
            local_objective: "Read the prioritized local source files and extract the strongest issue candidates.",
            local_display_name: "Curator Local Sources",
            external_node_id: "curate-issue-external-research",
            external_agent_id: "curator-external",
            external_title: "Research Issue",
            external_objective: "Gather timely external signals that should influence this week's issue.",
            external_display_name: "Curator External",
            final_title: "Curate Issue",
            final_objective: "Curate the best items for this week's issue from the staged research handoffs.",
        }),
        "sales-prospecting-team" => Some(SplitResearchTemplateConfig {
            template_id: "sales-prospecting-team",
            final_node_id: "research-account",
            final_agent_id: "account-research",
            discover_node_id: "research-account-discover",
            discover_agent_id: "account-discover",
            discover_title: "Discover Account Sources",
            discover_objective: "Identify the source corpus that should guide account research.",
            discover_display_name: "Account Discover",
            local_node_id: "research-account-local-sources",
            local_agent_id: "account-local-sources",
            local_title: "Read Account Sources",
            local_objective: "Read the prioritized local account and ICP files before drafting the account brief.",
            local_display_name: "Account Local Sources",
            external_node_id: "research-account-external-research",
            external_agent_id: "account-external",
            external_title: "Research Account Externally",
            external_objective: "Gather targeted external account context and buying signals to support the brief.",
            external_display_name: "Account External",
            final_title: "Research Account",
            final_objective: "Prepare the final account brief from the staged discovery, local evidence, and external research.",
        }),
        _ => None,
    }
}

fn studio_template_id(automation: &AutomationV2Spec) -> Option<String> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("studio"))
        .and_then(Value::as_object)
        .and_then(|studio| {
            studio
                .get("template_id")
                .or_else(|| studio.get("templateId"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn split_research_stage_metadata(
    title: &str,
    role: &str,
    prompt: String,
    research_stage: &str,
    output_path: Option<&str>,
    required_tools: &[&str],
    write_required: bool,
) -> Option<Value> {
    let mut builder = serde_json::Map::new();
    builder.insert("title".to_string(), json!(title));
    builder.insert("role".to_string(), json!(role));
    builder.insert("prompt".to_string(), json!(prompt));
    builder.insert("research_stage".to_string(), json!(research_stage));
    if let Some(path) = output_path {
        builder.insert("output_path".to_string(), json!(path));
    }
    if !required_tools.is_empty() {
        builder.insert("required_tools".to_string(), json!(required_tools));
    }
    if write_required {
        builder.insert("write_required".to_string(), json!(true));
    }
    let mut studio = serde_json::Map::new();
    studio.insert("research_stage".to_string(), json!(research_stage));
    if let Some(path) = output_path {
        studio.insert("output_path".to_string(), json!(path));
    }
    Some(json!({
        "builder": Value::Object(builder),
        "studio": Value::Object(studio),
    }))
}

fn migrated_stage_agent(
    base: &AutomationAgentProfile,
    agent_id: &str,
    display_name: &str,
    allowlist: &[&str],
) -> AutomationAgentProfile {
    let mut agent = base.clone();
    agent.agent_id = agent_id.to_string();
    agent.display_name = display_name.to_string();
    agent.template_id = None;
    agent.tool_policy.allowlist = config::channels::normalize_allowed_tools(
        allowlist.iter().map(|value| (*value).to_string()).collect(),
    );
    agent.tool_policy.denylist =
        config::channels::normalize_allowed_tools(agent.tool_policy.denylist.clone());
    agent
}

fn migrate_split_research_studio_metadata(metadata: &mut Value) {
    let Some(root) = metadata.as_object_mut() else {
        return;
    };
    let studio = root
        .entry("studio".to_string())
        .or_insert_with(|| json!({}));
    let Some(studio_obj) = studio.as_object_mut() else {
        return;
    };
    studio_obj.insert("version".to_string(), json!(2));
    studio_obj.insert("workflow_structure_version".to_string(), json!(2));
    studio_obj.remove("agent_drafts");
    studio_obj.remove("node_drafts");
    studio_obj.remove("node_layout");
}

pub(super) fn migrate_bundled_studio_research_split_automation(
    automation: &mut AutomationV2Spec,
) -> bool {
    let Some(template_id) = studio_template_id(automation) else {
        return false;
    };
    let Some(config) = split_research_template_config(&template_id) else {
        return false;
    };
    if automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == config.discover_node_id)
        || automation
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == config.final_node_id)
            .is_some_and(automation_node_is_research_finalize)
    {
        if let Some(metadata) = automation.metadata.as_mut() {
            migrate_split_research_studio_metadata(metadata);
        }
        return false;
    }
    let Some(final_node_index) = automation
        .flow
        .nodes
        .iter()
        .position(|node| node.node_id == config.final_node_id)
    else {
        return false;
    };
    let Some(base_agent) = automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == config.final_agent_id)
        .cloned()
    else {
        return false;
    };
    let existing_final_node = automation.flow.nodes[final_node_index].clone();
    let output_path = automation_node_required_output_path(&existing_final_node);
    let final_contract_kind = existing_final_node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.clone())
        .unwrap_or_else(|| "artifact".to_string());
    let final_is_brief_like = final_contract_kind.trim().eq_ignore_ascii_case("brief");
    let final_summary_guidance = existing_final_node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.summary_guidance.clone());
    let discover_prompt = "Enumerate the workspace, identify the relevant source corpus, and return a structured handoff with `workspace_inventory_summary`, `discovered_paths`, `priority_paths`, and `skipped_paths_initial`. If a curated source index such as `SOURCES.md` exists, read it first. Perform at least one concrete `read` before finishing, but read only enough to identify the corpus for the next stage. Do not write final workspace artifacts in this stage.".to_string();
    let local_prompt = "Use the upstream `source_inventory` handoff to decide which concrete local files to read. Perform concrete `read` calls, extract the product or market facts supported by those reads, and return a structured handoff with `read_paths`, `reviewed_facts`, `files_reviewed`, `files_not_reviewed`, and `citations_local`. Do not invent facts from filenames alone.".to_string();
    let external_prompt = "Use the upstream `source_inventory` and `local_source_notes` handoffs to guide targeted external research. Perform `websearch` and fetch result pages when snippets are not enough, then return `external_research_mode`, `queries_attempted`, `sources_reviewed`, `citations_external`, and `research_limitations`. If search is unavailable, record that limitation clearly instead of inventing evidence.".to_string();
    let final_prompt = match config.template_id {
        "marketing-content-pipeline" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth. Read `marketing-brief.md` from disk only as a fallback or verification step. Synthesize the final marketing brief from those handoffs instead of repeating discovery or fresh web research in this stage. Include a workspace source audit, audience, positioning, proof points with citations, `Files reviewed`, `Files not reviewed`, and `Web sources reviewed`, and clearly note any research limitations. In source-audit sections, list only exact concrete workspace-relative file paths or exact reviewed URLs; do not use directory names, wildcard paths, or glob patterns.".to_string(),
        "competitor-research-pipeline" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth for the final competitor scan. Separate observed evidence from inference, keep the scan current and signal-focused, and do not rerun discovery or fresh web research in this stage.".to_string(),
        "weekly-newsletter-builder" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs to curate the final issue. Turn them into the final shortlist and section order without repeating discovery or fresh web research in this stage.".to_string(),
        "sales-prospecting-team" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth for the final account brief. Separate observed facts from hypotheses and do not rerun discovery or fresh web research in this stage.".to_string(),
        _ => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth and synthesize the final artifact without repeating discovery or fresh web research in this stage.".to_string(),
    };

    let discover_node = AutomationFlowNode {
        node_id: config.discover_node_id.to_string(),
        agent_id: config.discover_agent_id.to_string(),
        objective: config.discover_objective.to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("local_research".to_string()),
                required_tools: vec!["read".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec![
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff in the final response instead of writing workspace files."
                    .to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.discover_title,
            "watcher",
            discover_prompt,
            "research_discover",
            None,
            &["glob", "read"],
            false,
        ),
    };
    let local_node = AutomationFlowNode {
        node_id: config.local_node_id.to_string(),
        agent_id: config.local_agent_id.to_string(),
        objective: config.local_objective.to_string(),
        depends_on: vec![config.discover_node_id.to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: config.discover_node_id.to_string(),
            alias: "source_inventory".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("local_research".to_string()),
                required_tools: vec!["read".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec!["concrete_reads".to_string()],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff backed by concrete local file reads.".to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.local_title,
            "watcher",
            local_prompt,
            "research_local_sources",
            None,
            &["read"],
            false,
        ),
    };
    let external_node = AutomationFlowNode {
        node_id: config.external_node_id.to_string(),
        agent_id: config.external_agent_id.to_string(),
        objective: config.external_objective.to_string(),
        depends_on: vec![
            config.discover_node_id.to_string(),
            config.local_node_id.to_string(),
        ],
        input_refs: vec![
            AutomationFlowInputRef {
                from_step_id: config.discover_node_id.to_string(),
                alias: "source_inventory".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: config.local_node_id.to_string(),
                alias: "local_source_notes".to_string(),
            },
        ],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("external_research".to_string()),
                required_tools: vec!["websearch".to_string()],
                required_evidence: vec!["external_sources".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec!["successful_web_research".to_string()],
                retry_on_missing: vec![
                    "external_sources".to_string(),
                    "successful_web_research".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff describing external research findings or limitations."
                    .to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.external_title,
            "watcher",
            external_prompt,
            "research_external_sources",
            None,
            &["websearch", "webfetch", "read"],
            false,
        ),
    };
    let mut final_node = existing_final_node.clone();
    final_node.objective = config.final_objective.to_string();
    final_node.depends_on = vec![
        config.discover_node_id.to_string(),
        config.local_node_id.to_string(),
        config.external_node_id.to_string(),
    ];
    final_node.input_refs = vec![
        AutomationFlowInputRef {
            from_step_id: config.discover_node_id.to_string(),
            alias: "source_inventory".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: config.local_node_id.to_string(),
            alias: "local_source_notes".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: config.external_node_id.to_string(),
            alias: "external_research".to_string(),
        },
    ];
    final_node.stage_kind = Some(AutomationNodeStageKind::Workstream);
    final_node.output_contract = Some(AutomationFlowOutputContract {
        kind: final_contract_kind,
        validator: existing_final_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.validator)
            .or(if final_is_brief_like {
                Some(crate::AutomationOutputValidatorKind::ResearchBrief)
            } else {
                None
            }),
        enforcement: Some(crate::AutomationOutputEnforcement {
            validation_profile: Some("research_synthesis".to_string()),
            required_tools: Vec::new(),
            required_evidence: vec![
                "local_source_reads".to_string(),
                "external_sources".to_string(),
            ],
            required_sections: if final_is_brief_like {
                vec!["citations".to_string()]
            } else {
                Vec::new()
            },
            prewrite_gates: Vec::new(),
            retry_on_missing: if final_is_brief_like {
                vec![
                    "local_source_reads".to_string(),
                    "external_sources".to_string(),
                    "citations".to_string(),
                ]
            } else {
                vec![
                    "local_source_reads".to_string(),
                    "external_sources".to_string(),
                ]
            },
            terminal_on: vec![
                "tool_unavailable".to_string(),
                "repair_budget_exhausted".to_string(),
            ],
            repair_budget: Some(5),
            session_text_recovery: Some("require_prewrite_satisfied".to_string()),
        }),
        schema: existing_final_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.schema.clone()),
        summary_guidance: final_summary_guidance,
    });
    final_node.metadata = split_research_stage_metadata(
        config.final_title,
        "watcher",
        final_prompt,
        "research_finalize",
        output_path.as_deref(),
        &[],
        output_path.is_some(),
    );

    let mut new_nodes = Vec::with_capacity(automation.flow.nodes.len() + 3);
    let mut inserted = false;
    for node in automation.flow.nodes.clone() {
        if node.node_id == config.final_node_id {
            new_nodes.push(discover_node.clone());
            new_nodes.push(local_node.clone());
            new_nodes.push(external_node.clone());
            new_nodes.push(final_node.clone());
            inserted = true;
        } else if node.node_id != config.discover_node_id
            && node.node_id != config.local_node_id
            && node.node_id != config.external_node_id
        {
            new_nodes.push(node);
        }
    }
    if !inserted {
        return false;
    }
    automation.flow.nodes = new_nodes;

    for candidate in [
        migrated_stage_agent(
            &base_agent,
            config.discover_agent_id,
            config.discover_display_name,
            &["glob", "read"],
        ),
        migrated_stage_agent(
            &base_agent,
            config.local_agent_id,
            config.local_display_name,
            &["read"],
        ),
        migrated_stage_agent(
            &base_agent,
            config.external_agent_id,
            config.external_display_name,
            &["websearch", "webfetch", "read"],
        ),
    ] {
        if !automation
            .agents
            .iter()
            .any(|agent| agent.agent_id == candidate.agent_id)
        {
            automation.agents.push(candidate);
        }
    }
    if let Some(final_agent) = automation
        .agents
        .iter_mut()
        .find(|agent| agent.agent_id == config.final_agent_id)
    {
        final_agent.tool_policy.allowlist = config::channels::normalize_allowed_tools(vec![
            "read".to_string(),
            "write".to_string(),
        ]);
    }
    if let Some(metadata) = automation.metadata.as_mut() {
        migrate_split_research_studio_metadata(metadata);
    } else {
        automation.metadata = Some(json!({
            "studio": {
                "template_id": config.template_id,
                "version": 2,
                "workflow_structure_version": 2
            }
        }));
    }
    true
}

fn automation_phase_execution_mode_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, String> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .filter_map(|phase| {
                    let phase_id = phase.get("phase_id").and_then(Value::as_str)?.trim();
                    if phase_id.is_empty() {
                        return None;
                    }
                    let mode = phase
                        .get("execution_mode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("soft");
                    Some((phase_id.to_string(), mode.to_string()))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_current_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<(String, usize, String)> {
    let phase_rank = automation_phase_rank_map(automation);
    if phase_rank.is_empty() {
        return None;
    }
    let phase_modes = automation_phase_execution_mode_map(automation);
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| !completed.contains(&node.node_id))
        .filter_map(|node| {
            automation_node_builder_metadata(node, "phase_id").and_then(|phase_id| {
                phase_rank
                    .get(&phase_id)
                    .copied()
                    .map(|rank| (phase_id, rank))
            })
        })
        .min_by_key(|(_, rank)| *rank)
        .map(|(phase_id, rank)| {
            let mode = phase_modes
                .get(&phase_id)
                .cloned()
                .unwrap_or_else(|| "soft".to_string());
            (phase_id, rank, mode)
        })
}

pub(crate) fn automation_phase_rank_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, usize> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .enumerate()
                .filter_map(|(index, phase)| {
                    phase
                        .get("phase_id")
                        .and_then(Value::as_str)
                        .map(|phase_id| (phase_id.to_string(), index))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_node_sort_key(
    node: &AutomationFlowNode,
    phase_rank: &std::collections::HashMap<String, usize>,
    current_open_phase_rank: Option<usize>,
) -> (usize, usize, i32, String) {
    let phase_order = automation_node_builder_metadata(node, "phase_id")
        .as_ref()
        .and_then(|phase_id| phase_rank.get(phase_id))
        .copied()
        .unwrap_or(usize::MAX / 2);
    let open_phase_bias = current_open_phase_rank
        .map(|open_rank| usize::from(phase_order != open_rank))
        .unwrap_or(0);
    (
        open_phase_bias,
        phase_order,
        -automation_node_builder_priority(node),
        node.node_id.clone(),
    )
}

pub(crate) fn automation_filter_runnable_by_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    runnable: Vec<AutomationFlowNode>,
) -> Vec<AutomationFlowNode> {
    let Some((_, open_rank, _)) = automation_current_open_phase(automation, run) else {
        return runnable;
    };
    let phase_rank = automation_phase_rank_map(automation);
    let in_open_phase = runnable
        .iter()
        .filter(|node| {
            automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied()
                == Some(open_rank)
        })
        .cloned()
        .collect::<Vec<_>>();
    if in_open_phase.is_empty() {
        runnable
    } else {
        in_open_phase
    }
}

fn automation_plan_package(automation: &AutomationV2Spec) -> Option<compiler_api::PlanPackage> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("plan_package"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn automation_routine_for_node<'a>(
    plan: &'a compiler_api::PlanPackage,
    node_id: &str,
) -> Option<&'a compiler_api::RoutinePackage> {
    plan.routine_graph
        .iter()
        .find(|routine| routine.steps.iter().any(|step| step.step_id == node_id))
}

fn routine_is_complete(
    routine: &compiler_api::RoutinePackage,
    completed_nodes: &HashSet<String>,
) -> bool {
    routine
        .steps
        .iter()
        .all(|step| completed_nodes.contains(&step.step_id))
}

fn automation_node_routine_dependencies_blocked(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
) -> bool {
    let Some(plan) = automation_plan_package(automation) else {
        return false;
    };
    let Some(routine) = automation_routine_for_node(&plan, &node.node_id) else {
        return false;
    };
    let completed_nodes = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    routine.dependencies.iter().any(|dependency| {
        if !matches!(dependency.mode, compiler_api::DependencyMode::Hard) {
            return false;
        }
        plan.routine_graph
            .iter()
            .find(|candidate_routine| candidate_routine.routine_id == dependency.routine_id)
            .is_some_and(|candidate_routine| {
                !routine_is_complete(candidate_routine, &completed_nodes)
            })
    })
}

pub(crate) fn automation_filter_runnable_by_routine_dependencies(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    runnable: Vec<AutomationFlowNode>,
) -> Vec<AutomationFlowNode> {
    runnable
        .into_iter()
        .filter(|node| !automation_node_routine_dependencies_blocked(automation, run, node))
        .collect()
}

fn normalize_write_scope_entries(scope: Option<String>) -> Vec<String> {
    let Some(scope) = scope else {
        return vec!["__repo__".to_string()];
    };
    let entries = scope
        .split(|ch| matches!(ch, ',' | '\n' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('/').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        vec!["__repo__".to_string()]
    } else {
        entries
    }
}

fn write_scope_entries_conflict(left: &[String], right: &[String]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            a == "__repo__"
                || b == "__repo__"
                || a == b
                || a == "."
                || b == "."
                || a == "*"
                || b == "*"
                || a.starts_with(&format!("{}/", b))
                || b.starts_with(&format!("{}/", a))
        })
    })
}

pub(crate) fn automation_filter_runnable_by_write_scope_conflicts(
    runnable: Vec<AutomationFlowNode>,
    max_parallel: usize,
) -> Vec<AutomationFlowNode> {
    if max_parallel <= 1 {
        return runnable.into_iter().take(1).collect();
    }
    let mut selected = Vec::new();
    let mut selected_scopes = Vec::<Vec<String>>::new();
    for node in runnable {
        let is_code = automation_node_is_code_workflow(&node);
        let scope_entries = if is_code {
            normalize_write_scope_entries(automation_node_write_scope(&node))
        } else {
            Vec::new()
        };
        let conflicts = is_code
            && selected.iter().enumerate().any(|(index, existing)| {
                automation_node_is_code_workflow(existing)
                    && write_scope_entries_conflict(&scope_entries, &selected_scopes[index])
            });
        if conflicts {
            continue;
        }
        if is_code {
            selected_scopes.push(scope_entries);
        } else {
            selected_scopes.push(Vec::new());
        }
        selected.push(node);
        if selected.len() >= max_parallel {
            break;
        }
    }
    selected
}

pub(crate) fn automation_blocked_nodes(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Vec<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let pending = run
        .checkpoint
        .pending_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let phase_rank = automation_phase_rank_map(automation);
    let current_open_phase = automation_current_open_phase(automation, run);
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| pending.contains(&node.node_id))
        .filter_map(|node| {
            let missing_deps = node.depends_on.iter().any(|dep| !completed.contains(dep));
            if missing_deps {
                return Some(node.node_id.clone());
            }
            if automation_node_routine_dependencies_blocked(automation, run, node) {
                return Some(node.node_id.clone());
            }
            let Some((_, open_rank, mode)) = current_open_phase.as_ref() else {
                return None;
            };
            if mode != "barrier" {
                return None;
            }
            let node_phase_rank = automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied();
            if node_phase_rank.is_some_and(|rank| rank > *open_rank) {
                return Some(node.node_id.clone());
            }
            None
        })
        .collect::<Vec<_>>()
}

pub(crate) fn record_automation_open_phase_event(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    let Some((phase_id, phase_rank, execution_mode)) =
        automation_current_open_phase(automation, run)
    else {
        return;
    };
    let last_recorded = run
        .checkpoint
        .lifecycle_history
        .iter()
        .rev()
        .find(|entry| entry.event == "phase_opened")
        .and_then(|entry| entry.metadata.as_ref())
        .and_then(|metadata| metadata.get("phase_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if last_recorded.as_deref() == Some(phase_id.as_str()) {
        return;
    }
    record_automation_lifecycle_event_with_metadata(
        run,
        "phase_opened",
        Some(format!("phase `{}` is now open", phase_id)),
        None,
        Some(json!({
            "phase_id": phase_id,
            "phase_rank": phase_rank,
            "execution_mode": execution_mode,
        })),
    );
}

pub fn refresh_automation_runtime_state(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    run.checkpoint.blocked_nodes = automation_blocked_nodes(automation, run);
    record_automation_open_phase_event(automation, run);
}

fn automation_mission_milestones(automation: &AutomationV2Spec) -> Vec<Value> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("milestones"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn completed_mission_milestones(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> std::collections::HashSet<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation_mission_milestones(automation)
        .iter()
        .filter_map(|milestone| {
            let milestone_id = milestone
                .get("milestone_id")
                .and_then(Value::as_str)?
                .trim();
            if milestone_id.is_empty() {
                return None;
            }
            let required = milestone
                .get("required_stage_ids")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (!required.is_empty()
                && required
                    .iter()
                    .all(|stage_id| completed.contains(*stage_id)))
            .then_some(milestone_id.to_string())
        })
        .collect()
}

pub(crate) fn record_milestone_promotions(
    automation: &AutomationV2Spec,
    row: &mut AutomationV2RunRecord,
    promoted_by_node_id: &str,
) {
    let already_recorded = row
        .checkpoint
        .lifecycle_history
        .iter()
        .filter(|entry| entry.event == "milestone_promoted")
        .filter_map(|entry| {
            entry.metadata.as_ref().and_then(|metadata| {
                metadata
                    .get("milestone_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .collect::<std::collections::HashSet<_>>();
    let completed = completed_mission_milestones(automation, row);
    for milestone in automation_mission_milestones(automation) {
        let milestone_id = milestone
            .get("milestone_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if milestone_id.is_empty()
            || !completed.contains(milestone_id)
            || already_recorded.contains(milestone_id)
        {
            continue;
        }
        let title = milestone
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or(milestone_id);
        let phase_id = milestone
            .get("phase_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let required_stage_ids = milestone
            .get("required_stage_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        record_automation_lifecycle_event_with_metadata(
            row,
            "milestone_promoted",
            Some(format!("milestone `{title}` promoted")),
            None,
            Some(json!({
                "milestone_id": milestone_id,
                "title": title,
                "phase_id": phase_id,
                "required_stage_ids": required_stage_ids,
                "promoted_by_node_id": promoted_by_node_id,
            })),
        );
    }
}

pub fn collect_automation_descendants(
    automation: &AutomationV2Spec,
    root_ids: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut descendants = root_ids.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for node in &automation.flow.nodes {
            if descendants.contains(&node.node_id) {
                continue;
            }
            if node.depends_on.iter().any(|dep| descendants.contains(dep)) {
                descendants.insert(node.node_id.clone());
                changed = true;
            }
        }
    }
    descendants
}

/// Returns all transitive ancestors of `node_id` (i.e. every node that
/// `node_id` directly or indirectly depends on), not including `node_id`
/// itself.
pub fn collect_automation_ancestors(
    automation: &AutomationV2Spec,
    node_id: &str,
) -> std::collections::HashSet<String> {
    let mut ancestors = std::collections::HashSet::new();
    let mut queue = vec![node_id.to_string()];
    while let Some(current_id) = queue.pop() {
        if let Some(node) = automation
            .flow
            .nodes
            .iter()
            .find(|n| n.node_id == current_id)
        {
            for dep in &node.depends_on {
                if ancestors.insert(dep.clone()) {
                    queue.push(dep.clone());
                }
            }
        }
    }
    ancestors
}

pub(crate) fn render_automation_v2_prompt(
    automation: &AutomationV2Spec,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
    requested_tools: &[String],
    template_system_prompt: Option<&str>,
    standup_report_path: Option<&str>,
    memory_project_id: Option<&str>,
) -> String {
    render_automation_v2_prompt_with_options(
        automation,
        workspace_root,
        run_id,
        node,
        attempt,
        agent,
        upstream_inputs,
        requested_tools,
        template_system_prompt,
        standup_report_path,
        memory_project_id,
        AutomationPromptRenderOptions::default(),
    )
}

fn render_automation_v2_prompt_with_options(
    automation: &AutomationV2Spec,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
    requested_tools: &[String],
    template_system_prompt: Option<&str>,
    standup_report_path: Option<&str>,
    memory_project_id: Option<&str>,
    options: AutomationPromptRenderOptions,
) -> String {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("structured_json");
    let normalized_upstream_inputs = upstream_inputs
        .iter()
        .map(|input| {
            let mut normalized_input = input.clone();
            if let Some(output) = input.get("output") {
                if let Some(object) = normalized_input.as_object_mut() {
                    object.insert(
                        "output".to_string(),
                        normalize_upstream_research_output_paths(
                            workspace_root,
                            Some(run_id),
                            output,
                        ),
                    );
                }
            }
            normalized_input
        })
        .collect::<Vec<_>>();
    let mut sections = Vec::new();
    if let Some(system_prompt) = template_system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Template system prompt:\n{}", system_prompt));
    }
    if let Some(mission) = automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("mission"))
    {
        let mission_title = mission
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(automation.name.as_str());
        let mission_goal = mission
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let success_criteria = mission
            .get("success_criteria")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(|row| format!("- {}", row.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let shared_context = mission
            .get("shared_context")
            .and_then(Value::as_str)
            .unwrap_or_default();
        sections.push(format!(
            "Mission Brief:\nTitle: {mission_title}\nGoal: {mission_goal}\nShared context: {shared_context}\nSuccess criteria:\n{}",
            if success_criteria.is_empty() {
                "- none provided".to_string()
            } else {
                success_criteria
            }
        ));
    }
    sections.push(format!(
        "Automation ID: {}\nRun ID: {}\nNode ID: {}\nAgent: {}\nObjective: {}\nOutput contract kind: {}",
        automation.automation_id, run_id, node.node_id, agent.display_name, node.objective, contract_kind
    ));
    if let Some(contract) = node.output_contract.as_ref() {
        let schema = contract
            .schema
            .as_ref()
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".to_string());
        let guidance = contract.summary_guidance.as_deref().unwrap_or("none");
        sections.push(format!(
            "Output Contract:\nKind: {}\nSummary guidance: {}\nSchema:\n{}",
            contract.kind, guidance, schema
        ));
    }
    if let Some(builder) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
    {
        let local_title = builder
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(node.node_id.as_str());
        let local_prompt = builder
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let local_role = builder
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        sections.push(format!(
            "Local Assignment:\nTitle: {local_title}\nRole: {local_role}\nInstructions: {local_prompt}"
        ));
    }
    if let Some(inputs) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("inputs"))
        .filter(|value| !value.is_null())
    {
        let rendered = serde_json::to_string_pretty(inputs).unwrap_or_else(|_| inputs.to_string());
        sections.push(format!(
            "Node Inputs:\n- Use these values directly when they satisfy the objective.\n- Do not search `/tmp`, shell history, or undeclared temp files for duplicate copies of these inputs.\n{}",
            rendered
                .lines()
                .map(|line| format!("  {}", line))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let execution_mode = automation_node_execution_mode(node, workspace_root);
    sections.push(format!(
        "Execution Policy:\n- Mode: `{}`.\n- Use only declared workflow artifact paths.\n- Keep status and blocker notes in the response JSON, not as placeholder file contents.",
        execution_mode
    ));
    let quality_mode = enforcement::automation_node_quality_mode(node);
    sections.push(format!(
        "Workflow Quality Mode:\n- Mode: `{}`.\n- Strict mode enforces evidence-backed synthesis and deterministic delivery bodies.\n- Legacy mode is a rollback path that relaxes the newest synthesis gates.",
        quality_mode.stable_key()
    ));
    if automation_node_is_code_workflow(node) {
        let task_kind =
            automation_node_task_kind(node).unwrap_or_else(|| "code_change".to_string());
        let project_backlog_tasks = automation_node_projects_backlog_tasks(node);
        let task_id = automation_node_task_id(node).unwrap_or_else(|| "unassigned".to_string());
        let repo_root = automation_node_repo_root(node).unwrap_or_else(|| ".".to_string());
        let write_scope =
            automation_node_write_scope(node).unwrap_or_else(|| "repo-scoped edits".to_string());
        let acceptance_criteria = automation_node_acceptance_criteria(node)
            .unwrap_or_else(|| "satisfy the declared coding task acceptance criteria".to_string());
        let task_dependencies =
            automation_node_task_dependencies(node).unwrap_or_else(|| "none declared".to_string());
        let verification_state =
            automation_node_verification_state(node).unwrap_or_else(|| "pending".to_string());
        let task_owner =
            automation_node_task_owner(node).unwrap_or_else(|| "unclaimed".to_string());
        let verification_command =
            automation_node_verification_command(node).unwrap_or_else(|| {
                "run the most relevant repo-local build, test, or lint commands".to_string()
            });
        sections.push(format!(
            "Coding Task Context:\n- Task id: `{}`.\n- Task kind: `{}`.\n- Repo root: `{}`.\n- Declared write scope: {}.\n- Acceptance criteria: {}.\n- Backlog dependencies: {}.\n- Verification state: {}.\n- Preferred owner: {}.\n- Verification expectation: {}.\n- Projects backlog tasks: {}.\n- Prefer repository edits plus a concise handoff artifact, not placeholder file rewrites.\n- Use `bash` for verification commands when tool access allows it.",
            task_id, task_kind, repo_root, write_scope, acceptance_criteria, task_dependencies, verification_state, task_owner, verification_command, if project_backlog_tasks { "yes" } else { "no" }
        ));
        sections.push(
            "Code Agent Contract:\n- Follow the deterministic loop: inspect -> patch -> apply -> test -> repair -> finalize.\n- Read before editing, prefer diffs for existing files, and keep changes within the declared write scope.\n- Use `apply_patch` for multi-line edits and `edit` for localized replacements whenever available; use `write` only for brand-new files or when a diff cannot express the change.\n- Run the declared verification command after applying changes. If it fails, read the failure output and repair the smallest root cause before retrying.\n- Do not claim completion until the patch has been applied and verification has succeeded or been explicitly waived by the runtime."
                .to_string(),
        );
    }
    let artifact_output_path = automation_node_required_output_path_for_run(node, Some(run_id));
    if let Some(output_path) = artifact_output_path.as_deref() {
        let output_rules = match execution_mode {
            "git_patch" => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `apply_patch` for multi-line source edits and `edit` for localized replacements.\n- Use `write` only for brand-new files or when patch/edit cannot express the change.\n- Do not replace an existing source file with a status note, preservation note, or placeholder summary.\n- Only write declared workflow artifact files.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
            "filesystem_patch" => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `edit` for existing-file changes.\n- Use `write` for brand-new files or as a last resort when an edit cannot express the change.\n- Do not replace an existing file with a status note, preservation note, or placeholder summary.\n- Only write declared workflow artifact files.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
            _ => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Use the `write` tool to create the full file contents.\n- Only write declared workflow artifact files; do not create auxiliary touch files, status files, marker files, or placeholder preservation notes.\n- Overwrite the declared output with the actual artifact contents for this run instead of preserving a prior placeholder.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
        };
        sections.push(output_rules);
    }
    if node.node_id == "collect_inputs" {
        let collect_inputs_output_path =
            automation_run_scoped_output_path(run_id, ".tandem/artifacts/collect-inputs.json")
                .unwrap_or_else(|| ".tandem/artifacts/collect-inputs.json".to_string());
        sections.push(
            format!(
                "Collect Inputs Contract:\n- Use `glob` to discover the workspace shape, but do not stop after discovery.\n- Read concrete workspace files that ground the project identity, product terminology, capabilities, proof points, and source corpus.\n- Write the grounded result to `{}` before finishing.\n- The final brief must reflect the current run's reads and discovery, not a generic recap or a reused placeholder.",
                collect_inputs_output_path
            ),
        );
    }
    if automation_node_web_research_expected(node) {
        let requested_has_websearch = requested_tools.iter().any(|tool| tool == "websearch");
        let requested_has_webfetch = requested_tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "webfetch" | "webfetch_html"));
        if requested_has_websearch {
            sections.push(
                "External Research Expectation:\n- Use `websearch` for current external evidence before finalizing the output file.\n- Use `webfetch` on concrete result URLs when search snippets are not enough.\n- Include only evidence you can support from local files or current web findings.\n- If `websearch` returns an authorization-required or unavailable result, treat external research as unavailable for this run, continue with local file reads, and note the web-research limitation instead of stopping."
                    .to_string(),
            );
        } else if requested_has_webfetch {
            sections.push(
                "External Research Expectation:\n- `websearch` is not available in this run.\n- Use `webfetch` only for concrete URLs already present in local sources or upstream handoffs.\n- If you cannot validate externally without search, record that limitation in the structured handoff and finish the node.\n- Do not ask the user for clarification or permission to continue; return the required JSON handoff for this run."
                    .to_string(),
            );
        } else {
            sections.push(
                "External Research Expectation:\n- No web research tool is available in this run.\n- Record the web-research limitation clearly in the structured handoff, continue with any allowed local reads, and finish without asking follow-up questions."
                    .to_string(),
            );
        }
    }
    let validator_kind = automation_output_validator_kind(node);
    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    if handoff_only_structured_json {
        sections.push(
            "Structured Handoff Expectation:\n- Return the requested structured JSON handoff in the final response body.\n- The final response body should contain JSON only: the handoff JSON, then the final compact JSON status object.\n- Do not include headings, bullets, markdown fences, prose explanations, or follow-up questions.\n- Do not stop after tool calls alone; include a machine-readable JSON object or array with the requested fields."
                .to_string(),
        );
    }
    let mut prompt = sections.join("\n\n");
    if !normalized_upstream_inputs.is_empty() {
        prompt.push_str("\n\nUpstream Inputs:");
        for input in &normalized_upstream_inputs {
            let alias = input
                .get("alias")
                .and_then(Value::as_str)
                .unwrap_or("input");
            let from_step_id = input
                .get("from_step_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let output = input
                .get("output")
                .map(|value| {
                    compact_automation_prompt_output_with_mode(value, options.summary_only_upstream)
                })
                .unwrap_or(Value::Null);
            let rendered =
                serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string());
            prompt.push_str(&format!(
                "\n- {}\n  from_step_id: {}\n  output:\n{}",
                alias,
                from_step_id,
                rendered
                    .lines()
                    .map(|line| format!("    {}", line))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    if automation_node_is_research_finalize(node) {
        if let Some(summary) =
            render_research_finalize_upstream_summary(&normalized_upstream_inputs)
        {
            prompt.push_str("\n\n");
            prompt.push_str(&summary);
        }
    }
    if let Some(summary) = render_upstream_synthesis_guidance(node, &normalized_upstream_inputs) {
        prompt.push_str("\n\n");
        prompt.push_str(&summary);
    }
    if automation_node_requires_email_delivery(node) {
        prompt.push_str(
            "\n\nDelivery rules:\n- Prefer inline email body delivery by default.\n- Only include an email attachment when upstream inputs contain a concrete attachment artifact with a non-empty s3key or upload result.\n- Never send an attachment parameter with an empty or null s3key.\n- If no attachment artifact exists, omit the attachment parameter entirely.",
        );
        if let Some(deterministic_body) =
            render_deterministic_delivery_body(&normalized_upstream_inputs)
        {
            prompt.push_str("\n\n");
            prompt.push_str(&deterministic_body);
        }
        let delivery_target =
            automation_node_delivery_target(node).unwrap_or_else(|| "missing".to_string());
        let content_type =
            automation_node_email_content_type(node).unwrap_or_else(|| "text/html".to_string());
        let inline_body_only = automation_node_inline_body_only(node).unwrap_or(true);
        let attachments_allowed = automation_node_allows_attachments(node).unwrap_or(false);
        prompt.push_str(&format!(
            "\n\nDelivery target:\n- Method: `email`\n- Recipient: `{}`\n- Content-Type: `{}`\n- Inline body only: `{}`\n- Attachments allowed: `{}`\n- Treat this delivery target as authoritative for this run.\n- Do not say the recipient is missing when it is listed above.\n- Do not mark the node completed unless you actually execute an email draft or send tool.",
            delivery_target,
            content_type,
            inline_body_only,
            attachments_allowed
        ));
    }
    if let Some(report_path) = standup_report_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nStandup report path:\n- Write the final markdown report to `{}` relative to the workspace root.\n- Use the `write` tool for the report.\n- The report must remain inside the workspace.",
            report_path
        ));
    }
    if let Some(project_id) = memory_project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nMemory search scope:\n- `memory_search` defaults to the current session, current project, and global memory.\n- Current project_id: `{}`.\n- Use `tier: \"project\"` when you need recall limited to this workspace.\n- Use workspace files via `glob`, `grep`, and `read` when memory is sparse or stale.",
            project_id
        ));
    }
    let enforce_completed_first_attempt = (validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || !automation_node_required_tools(node).is_empty()
        || handoff_only_structured_json)
        && attempt <= 1;
    if enforce_completed_first_attempt {
        if automation_node_required_output_path(node).is_some() {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the write tool actually created the output file.",
            );
        } else {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the required structured handoff was actually returned in the final response.",
            );
        }
    } else {
        if handoff_only_structured_json {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include the required structured handoff JSON in the response body before the final compact status object.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim success unless the required structured handoff was actually returned in the final response.\n- Do not claim semantic success if the output is blocked or not approved.",
            );
        } else {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim semantic success if the output is blocked or not approved.",
            );
        }
    }
    prompt
}

fn truncate_automation_prompt_text(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let truncated = trimmed.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}

fn compact_automation_prompt_content(content: &Value, summary_only: bool) -> Value {
    let Some(object) = content.as_object() else {
        return content.clone();
    };
    let mut compact = serde_json::Map::new();
    if let Some(path) = object.get("path").cloned().filter(|value| !value.is_null()) {
        compact.insert("path".to_string(), path);
    }
    if let Some(handoff) = object
        .get("structured_handoff")
        .cloned()
        .filter(|value| !value.is_null())
    {
        compact.insert("structured_handoff".to_string(), handoff);
        return Value::Object(compact);
    }
    let candidate_text = object
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| object.get("raw_text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(text) = candidate_text {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if summary_only {
                compact.insert("data_summary".to_string(), summarize_json_keys(&parsed));
            } else {
                compact.insert("data".to_string(), parsed);
            }
        } else {
            compact.insert(
                "text".to_string(),
                json!(truncate_automation_prompt_text(
                    text,
                    if summary_only { 800 } else { 4000 }
                )),
            );
        }
    }
    Value::Object(compact)
}

fn compact_automation_prompt_output_with_mode(output: &Value, summary_only: bool) -> Value {
    let Some(object) = output.as_object() else {
        return output.clone();
    };
    let mut compact = serde_json::Map::new();
    for key in [
        "status",
        "phase",
        "contract_kind",
        "summary",
        "blocked_reason",
        "workflow_class",
    ] {
        if let Some(value) = object.get(key).cloned().filter(|value| !value.is_null()) {
            compact.insert(key.to_string(), value);
        }
    }
    if let Some(validator_summary) = object.get("validator_summary").and_then(Value::as_object) {
        let mut validator = serde_json::Map::new();
        for key in [
            "kind",
            "outcome",
            "warning_count",
            "warning_requirements",
            "unmet_requirements",
        ] {
            if let Some(value) = validator_summary
                .get(key)
                .cloned()
                .filter(|value| !value.is_null())
            {
                validator.insert(key.to_string(), value);
            }
        }
        if !validator.is_empty() {
            compact.insert("validator_summary".to_string(), Value::Object(validator));
        }
    }
    if let Some(artifact_validation) = object.get("artifact_validation").and_then(Value::as_object)
    {
        let mut validation = serde_json::Map::new();
        for key in [
            "accepted_artifact_path",
            "accepted_candidate_source",
            "validation_outcome",
            "validation_profile",
            "warning_count",
            "warning_requirements",
            "unmet_requirements",
            "semantic_block_reason",
            "rejected_artifact_reason",
        ] {
            if let Some(value) = artifact_validation
                .get(key)
                .cloned()
                .filter(|value| !value.is_null())
            {
                validation.insert(key.to_string(), value);
            }
        }
        if !validation.is_empty() {
            compact.insert("artifact_validation".to_string(), Value::Object(validation));
        }
    }
    if let Some(content) = object.get("content") {
        let compact_content = compact_automation_prompt_content(content, summary_only);
        if compact_content
            .as_object()
            .is_some_and(|value| !value.is_empty())
        {
            compact.insert("content".to_string(), compact_content);
        }
    }
    Value::Object(compact)
}

pub(crate) fn render_automation_repair_brief(
    node: &AutomationFlowNode,
    prior_output: Option<&Value>,
    attempt: u32,
    max_attempts: u32,
) -> Option<String> {
    if attempt <= 1 {
        return None;
    }
    let prior_output = prior_output?;
    if !automation_output_needs_repair(prior_output) {
        return None;
    }

    let validator_summary = prior_output.get("validator_summary");
    let artifact_validation = prior_output.get("artifact_validation");
    let tool_telemetry = prior_output.get("tool_telemetry");
    let validator_outcome = validator_summary
        .and_then(|value| value.get("outcome"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let unmet_requirements_from_summary = validator_summary
        .and_then(|value| value.get("unmet_requirements"))
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
    let is_upstream_passed = validator_outcome
        .is_some_and(|outcome| outcome.eq_ignore_ascii_case("passed"))
        && unmet_requirements_from_summary.is_empty();
    if is_upstream_passed {
        return None;
    }
    let reason = validator_summary
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("the previous attempt did not satisfy the runtime validator");
    let unmet_requirements = unmet_requirements_from_summary;
    let blocking_classification = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unspecified");
    let required_next_tool_actions = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
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
    let tools_offered = tool_telemetry
        .and_then(|value| value.get("requested_tools"))
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
    let tools_executed = tool_telemetry
        .and_then(|value| value.get("executed_tools"))
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
    let unreviewed_relevant_paths = artifact_validation
        .and_then(|value| value.get("unreviewed_relevant_paths"))
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
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(attempt.saturating_sub(1));
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| max_attempts.saturating_sub(attempt.saturating_sub(1)));

    let unmet_line = if unmet_requirements.is_empty() {
        "none recorded".to_string()
    } else {
        unmet_requirements.join(", ")
    };
    let tools_offered_line = if tools_offered.is_empty() {
        "none recorded".to_string()
    } else {
        tools_offered.join(", ")
    };
    let tools_executed_line = if tools_executed.is_empty() {
        "none recorded".to_string()
    } else {
        tools_executed.join(", ")
    };
    let unreviewed_line = if unreviewed_relevant_paths.is_empty() {
        "none recorded".to_string()
    } else {
        unreviewed_relevant_paths.join(", ")
    };
    let next_actions_line = if required_next_tool_actions.is_empty() {
        "none recorded".to_string()
    } else {
        required_next_tool_actions.join(" | ")
    };
    let code_workflow_line = if automation_node_is_code_workflow(node) {
        let verification_command =
            automation_node_verification_command(node).unwrap_or_else(|| {
                "run the most relevant repo-local build, test, or lint commands".to_string()
            });
        let write_scope =
            automation_node_write_scope(node).unwrap_or_else(|| "repo-scoped edits".to_string());
        format!(
            "\n- Code workflow repair path: inspect the touched files in `{}` first, patch with `edit` or `apply_patch` before any new `write`, then rerun verification with `{}` and fix the smallest failing root cause.",
            write_scope,
            verification_command
        )
    } else {
        String::new()
    };

    Some(format!(
        "Repair Brief:\n- Node `{}` is being retried because the previous attempt ended in `needs_repair`.\n- Previous validation reason: {}.\n- Unmet requirements: {}.\n- Blocking classification: {}.\n- Required next tool actions: {}.\n- Tools offered last attempt: {}.\n- Tools executed last attempt: {}.\n- Relevant files still unread or explicitly unreviewed: {}.\n- Previous repair attempt count: {}.\n- Remaining repair attempts after this run: {}{}.\n- For this retry, satisfy the unmet requirements before finalizing the artifact.\n- Do not write a blocked handoff unless the required tools were actually attempted and remained unavailable or failed.",
        node.node_id,
        reason,
        unmet_line,
        blocking_classification,
        next_actions_line,
        tools_offered_line,
        tools_executed_line,
        unreviewed_line,
        repair_attempt,
        repair_attempts_remaining.saturating_sub(1),
        code_workflow_line,
    ))
}

fn is_agent_standup_automation(automation: &AutomationV2Spec) -> bool {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("feature"))
        .and_then(Value::as_str)
        .map(|value| value == "agent_standup")
        .unwrap_or(false)
}

fn resolve_standup_report_path_template(automation: &AutomationV2Spec) -> Option<String> {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("standup"))
        .and_then(|value| value.get("report_path_template"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_standup_report_path_for_run(
    automation: &AutomationV2Spec,
    started_at_ms: u64,
) -> Option<String> {
    let template = resolve_standup_report_path_template(automation)?;
    if !template.contains("{{date}}") {
        return Some(template);
    }
    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(started_at_ms as i64)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m-%d")
        .to_string();
    Some(template.replace("{{date}}", &date))
}

fn automation_workspace_project_id(workspace_root: &str) -> String {
    tandem_core::workspace_project_id(workspace_root)
        .unwrap_or_else(|| "workspace-unknown".to_string())
}

fn merge_automation_agent_allowlist(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Vec<String> {
    let mut allowlist = if agent.tool_policy.allowlist.is_empty() {
        template
            .map(|value| value.capabilities.tool_allowlist.clone())
            .unwrap_or_default()
    } else {
        agent.tool_policy.allowlist.clone()
    };
    allowlist.sort();
    allowlist.dedup();
    allowlist
}

fn automation_node_output_extension(node: &AutomationFlowNode) -> Option<String> {
    automation_node_required_output_path(node)
        .as_deref()
        .and_then(|value| std::path::Path::new(value).extension())
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

pub(crate) fn automation_node_output_contract_kind(node: &AutomationFlowNode) -> Option<String> {
    node.output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn automation_node_task_kind(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_kind")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn automation_node_projects_backlog_tasks(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("project_backlog_tasks"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_task_id(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_repo_root(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "repo_root")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_write_scope(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "write_scope")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_acceptance_criteria(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "acceptance_criteria")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_task_dependencies(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_dependencies")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_task_owner(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_owner")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_is_code_workflow(node: &AutomationFlowNode) -> bool {
    if automation_node_task_kind(node)
        .as_deref()
        .is_some_and(|kind| matches!(kind, "code_change" | "repo_fix" | "implementation"))
    {
        return true;
    }
    if node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.validator)
        .is_some_and(|validator| validator == crate::AutomationOutputValidatorKind::CodePatch)
    {
        return true;
    }
    if automation_node_output_contract_kind(node).as_deref() == Some("code_patch") {
        return true;
    }
    let Some(extension) = automation_node_output_extension(node) else {
        return false;
    };
    let code_extensions = [
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "kts", "c", "cc", "cpp", "h",
        "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "zsh",
    ];
    code_extensions.contains(&extension.as_str())
}

pub(crate) fn automation_output_validator_kind(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputValidatorKind {
    if let Some(validator) = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.validator)
    {
        return validator;
    }
    if automation_node_is_code_workflow(node) {
        return crate::AutomationOutputValidatorKind::CodePatch;
    }
    match node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("brief") => crate::AutomationOutputValidatorKind::ResearchBrief,
        Some("review") => crate::AutomationOutputValidatorKind::ReviewDecision,
        Some("structured_json") => crate::AutomationOutputValidatorKind::StructuredJson,
        _ => crate::AutomationOutputValidatorKind::GenericArtifact,
    }
}

fn path_looks_like_source_file(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.replace('\\', "/");
    let path = std::path::Path::new(&normalized);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    if extension.as_deref().is_some_and(|extension| {
        [
            "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "kts", "c", "cc", "cpp", "h",
            "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "zsh", "toml", "yaml", "yml",
            "json",
        ]
        .contains(&extension)
    }) {
        return true;
    }
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|name| {
            matches!(
                name.as_str(),
                "cargo.toml"
                    | "cargo.lock"
                    | "package.json"
                    | "package-lock.json"
                    | "pnpm-lock.yaml"
                    | "tsconfig.json"
                    | "deno.json"
                    | "deno.jsonc"
                    | "jest.config.js"
                    | "jest.config.ts"
                    | "vite.config.ts"
                    | "vite.config.js"
                    | "webpack.config.js"
                    | "webpack.config.ts"
                    | "next.config.js"
                    | "next.config.mjs"
                    | "pyproject.toml"
                    | "requirements.txt"
                    | "makefile"
                    | "dockerfile"
            )
        })
}

fn workspace_has_git_repo(workspace_root: &str) -> bool {
    std::process::Command::new("git")
        .current_dir(workspace_root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn automation_node_execution_mode(node: &AutomationFlowNode, workspace_root: &str) -> &'static str {
    if !automation_node_is_code_workflow(node) {
        return "artifact_write";
    }
    if workspace_has_git_repo(workspace_root) {
        "git_patch"
    } else {
        "filesystem_patch"
    }
}

pub(crate) fn normalize_automation_requested_tools(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
) -> Vec<String> {
    let mut normalized = config::channels::normalize_allowed_tools(raw);
    let had_wildcard = normalized.iter().any(|tool| tool == "*");
    if had_wildcard {
        normalized.retain(|tool| tool != "*");
    }
    normalized.extend(automation_node_required_tools(node));
    match automation_node_execution_mode(node, workspace_root) {
        "git_patch" => {
            normalized.extend([
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "apply_patch".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ]);
        }
        "filesystem_patch" => {
            normalized.extend([
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ]);
        }
        _ => {
            if automation_node_required_output_path(node).is_some() {
                normalized.push("write".to_string());
            }
        }
    }
    if !node.input_refs.is_empty() {
        normalized.push("read".to_string());
    }
    let has_read = normalized.iter().any(|tool| tool == "read");
    let has_workspace_probe = normalized
        .iter()
        .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list"));
    if has_read && !has_workspace_probe {
        normalized.push("glob".to_string());
    }
    if automation_node_web_research_expected(node) {
        normalized.push("websearch".to_string());
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

fn automation_node_delivery_method(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/method")
                .or_else(|| value.pointer("/builder/delivery/method"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn automation_node_delivery_target(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/to")
                .or_else(|| value.pointer("/builder/delivery/to"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| extract_email_address_from_text(&node.objective))
}

fn extract_email_address_from_text(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|ch: char| {
                ch.is_ascii_punctuation() && ch != '@' && ch != '.' && ch != '_' && ch != '-'
            })
            .trim();
        if candidate.is_empty()
            || !candidate.contains('@')
            || candidate.starts_with('@')
            || candidate.ends_with('@')
        {
            return None;
        }
        let mut parts = candidate.split('@');
        let local = parts.next()?.trim();
        let domain = parts.next()?.trim();
        if parts.next().is_some()
            || local.is_empty()
            || domain.is_empty()
            || !domain.contains('.')
            || domain.starts_with('.')
            || domain.ends_with('.')
        {
            return None;
        }
        Some(candidate.to_string())
    })
}

fn automation_node_email_content_type(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/content_type")
                .or_else(|| value.pointer("/builder/delivery/content_type"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn automation_node_inline_body_only(node: &AutomationFlowNode) -> Option<bool> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/inline_body_only")
                .or_else(|| value.pointer("/builder/delivery/inline_body_only"))
        })
        .and_then(Value::as_bool)
}

fn automation_node_allows_attachments(node: &AutomationFlowNode) -> Option<bool> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/attachments")
                .or_else(|| value.pointer("/builder/delivery/attachments"))
        })
        .and_then(Value::as_bool)
}

pub(crate) fn automation_node_requires_email_delivery(node: &AutomationFlowNode) -> bool {
    if automation_node_delivery_method(node)
        .as_deref()
        .is_some_and(|method| method == "email")
    {
        return true;
    }
    if !automation_node_is_outbound_action(node) {
        return false;
    }
    let objective = node.objective.to_ascii_lowercase();
    let contains_phrase = [
        "send email",
        "send the email",
        "send by email",
        "send the report by email",
        "email the ",
        "email report",
        "draft email",
        "draft the email",
        "gmail draft",
        "gmail_send",
        "notify by email",
        "notify the operator by email",
    ]
    .iter()
    .any(|needle| objective.contains(needle));
    if contains_phrase {
        return true;
    }

    let mentions_email_channel = objective.contains("email")
        || objective.contains("gmail")
        || objective.contains("mail tool")
        || objective.contains("mail tools");
    let mentions_delivery_action = objective.contains("send")
        || objective.contains("draft")
        || objective.contains("notify")
        || objective.contains("deliver");

    mentions_email_channel && mentions_delivery_action
}

fn automation_tool_name_is_email_delivery(tool_name: &str) -> bool {
    let tokens = automation_tool_name_tokens(tool_name);
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "email"
                | "mail"
                | "gmail"
                | "outlook"
                | "smtp"
                | "imap"
                | "inbox"
                | "mailbox"
                | "mailer"
                | "exchange"
                | "sendgrid"
                | "mailgun"
                | "postmark"
                | "resend"
                | "ses"
        )
    })
}

fn discover_automation_tools_for_capability(
    capability_id: &str,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    let mut matches = available_tool_names
        .iter()
        .filter(|tool_name| automation_capability_matches_tool(capability_id, tool_name))
        .cloned()
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    matches
}

pub(crate) fn filter_requested_tools_to_available(
    requested_tools: Vec<String>,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    if requested_tools.iter().any(|tool| tool == "*") {
        return requested_tools;
    }
    requested_tools
        .into_iter()
        .filter(|tool| available_tool_names.contains(tool))
        .collect()
}

pub(crate) fn automation_requested_tools_for_node(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    let execution_mode = automation_node_execution_mode(node, workspace_root);
    let mut requested_tools = filter_requested_tools_to_available(
        normalize_automation_requested_tools(node, workspace_root, raw),
        available_tool_names,
    );
    for capability_id in automation_tool_capability_ids(node, execution_mode) {
        requested_tools.extend(discover_automation_tools_for_capability(
            &capability_id,
            available_tool_names,
        ));
    }
    requested_tools.sort();
    requested_tools.dedup();
    requested_tools
}

pub(crate) fn automation_node_prewrite_requirements(
    node: &AutomationFlowNode,
    requested_tools: &[String],
) -> Option<PrewriteRequirements> {
    let write_required = automation_node_required_output_path(node).is_some();
    if !write_required {
        return None;
    }
    let enforcement = automation_node_output_enforcement(node);
    let required_tools = enforcement.required_tools.clone();
    let web_research_expected = enforcement_requires_external_sources(&enforcement);
    let validation_profile = enforcement
        .validation_profile
        .as_deref()
        .unwrap_or("artifact_only");
    let workspace_inspection_required = requested_tools
        .iter()
        .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list" | "read"));
    let web_research_required =
        web_research_expected && requested_tools.iter().any(|tool| tool == "websearch");
    let brief_research_node = validation_profile == "local_research";
    let research_finalize = validation_profile == "research_synthesis";
    let has_required_read = required_tools.iter().any(|tool| tool == "read");
    let has_required_websearch = required_tools.iter().any(|tool| tool == "websearch");
    let has_any_required_tools = !required_tools.is_empty();
    let concrete_read_required = !research_finalize
        && ((brief_research_node || validation_profile == "local_research")
            || has_required_read
            || enforcement
                .prewrite_gates
                .iter()
                .any(|gate| gate == "concrete_reads"))
        && requested_tools.iter().any(|tool| tool == "read");
    let successful_web_research_required = !research_finalize
        && ((validation_profile == "external_research")
            || has_required_websearch
            || enforcement
                .prewrite_gates
                .iter()
                .any(|gate| gate == "successful_web_research"))
        && web_research_expected
        && requested_tools.iter().any(|tool| tool == "websearch");
    Some(PrewriteRequirements {
        workspace_inspection_required: workspace_inspection_required && !research_finalize,
        web_research_required: web_research_required && !research_finalize,
        concrete_read_required,
        successful_web_research_required,
        repair_on_unmet_requirements: brief_research_node || has_any_required_tools,
        coverage_mode: if brief_research_node {
            PrewriteCoverageMode::ResearchCorpus
        } else {
            PrewriteCoverageMode::None
        },
    })
}

fn validation_requirement_is_warning(profile: &str, requirement: &str) -> bool {
    match profile {
        "external_research" => matches!(
            requirement,
            "files_reviewed_missing"
                | "files_reviewed_not_backed_by_read"
                | "relevant_files_not_reviewed_or_skipped"
                | "web_sources_reviewed_missing"
                | "files_reviewed_contains_nonconcrete_paths"
        ),
        "research_synthesis" => matches!(
            requirement,
            "files_reviewed_missing"
                | "files_reviewed_not_backed_by_read"
                | "relevant_files_not_reviewed_or_skipped"
                | "web_sources_reviewed_missing"
                | "files_reviewed_contains_nonconcrete_paths"
                | "workspace_inspection_required"
        ),
        "local_research" => matches!(
            requirement,
            "files_reviewed_missing" | "relevant_files_not_reviewed_or_skipped"
        ),
        "artifact_only" => matches!(
            requirement,
            "editorial_substance_missing" | "markdown_structure_missing"
        ),
        _ => false,
    }
}

fn semantic_block_reason_for_requirements(unmet_requirements: &[String]) -> Option<String> {
    let has_unmet = |needle: &str| unmet_requirements.iter().any(|value| value == needle);
    if has_unmet("current_attempt_output_missing") {
        Some("required output was not created in the current attempt".to_string())
    } else if has_unmet("structured_handoff_missing") {
        Some("structured handoff was not returned in the final response".to_string())
    } else if has_unmet("workspace_inspection_required") {
        Some("structured handoff completed without required workspace inspection".to_string())
    } else if has_unmet("missing_successful_web_research") {
        Some("research completed without required current web research".to_string())
    } else if has_unmet("no_concrete_reads") || has_unmet("concrete_read_required") {
        Some(
            "research completed without concrete file reads or required source coverage"
                .to_string(),
        )
    } else if has_unmet("relevant_files_not_reviewed_or_skipped") {
        Some(
            "research completed without covering or explicitly skipping relevant discovered files"
                .to_string(),
        )
    } else if has_unmet("citations_missing") {
        Some("research completed without citation-backed claims".to_string())
    } else if has_unmet("web_sources_reviewed_missing") {
        Some("research completed without a web sources reviewed section".to_string())
    } else if has_unmet("files_reviewed_contains_nonconcrete_paths") {
        Some(
            "research artifact contains non-concrete paths (wildcards or directory placeholders) in source audit"
                .to_string(),
        )
    } else if has_unmet("files_reviewed_missing") || has_unmet("files_reviewed_not_backed_by_read")
    {
        Some("research completed without a source-backed files reviewed section".to_string())
    } else if has_unmet("upstream_evidence_not_synthesized") {
        Some(
            "final artifact does not adequately synthesize the available upstream evidence"
                .to_string(),
        )
    } else if has_unmet("markdown_structure_missing") {
        Some("editorial artifact is missing expected markdown structure".to_string())
    } else if has_unmet("editorial_substance_missing") {
        Some("editorial artifact is too weak or placeholder-like".to_string())
    } else {
        None
    }
}

fn resolve_automation_agent_model(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Option<ModelSpec> {
    if let Some(model) = agent
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(crate::app::routines::parse_model_spec)
    {
        return Some(model);
    }
    template
        .and_then(|value| value.default_model.as_ref())
        .and_then(crate::app::routines::parse_model_spec)
}

pub(crate) fn automation_node_inline_artifact_payload(node: &AutomationFlowNode) -> Option<Value> {
    if node.node_id != "collect_inputs" {
        return None;
    }
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("inputs"))
        .filter(|value| !value.is_null())
        .cloned()
}

pub(crate) fn write_automation_inline_artifact(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
    payload: &Value,
) -> anyhow::Result<(String, String)> {
    let resolved = resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            anyhow::anyhow!(
                "failed to create parent directory for required output `{}`: {}",
                output_path,
                error
            )
        })?;
    }
    let file_text = serde_json::to_string_pretty(payload)?;
    std::fs::write(&resolved, &file_text).map_err(|error| {
        anyhow::anyhow!(
            "failed to write deterministic workflow artifact `{}`: {}",
            output_path,
            error
        )
    })?;
    let display_path = resolved
        .strip_prefix(PathBuf::from(workspace_root))
        .ok()
        .and_then(|value| value.to_str().map(str::to_string))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| output_path.to_string());
    Ok((display_path, file_text))
}

fn automation_node_declared_output_path(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("output_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| automation_node_default_output_path(node))
}

pub(crate) fn automation_node_required_output_path_for_run(
    node: &AutomationFlowNode,
    run_id: Option<&str>,
) -> Option<String> {
    let output_path = automation_node_declared_output_path(node)?;
    run_id
        .and_then(|run_id| automation_run_scoped_output_path(run_id, &output_path))
        .or(Some(output_path))
}

pub fn automation_node_required_output_path(node: &AutomationFlowNode) -> Option<String> {
    automation_node_required_output_path_for_run(node, None)
}

fn automation_node_default_output_path(node: &AutomationFlowNode) -> Option<String> {
    let extension = match node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("structured_json")
    {
        "report_markdown" => {
            let format = node
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("format"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if format.eq_ignore_ascii_case("simple_html") {
                "html"
            } else {
                "md"
            }
        }
        "approval_gate" => return None,
        _ => "json",
    };
    let default_enabled = matches!(
        node.node_id.as_str(),
        "collect_inputs"
            | "research_sources"
            | "extract_pain_points"
            | "cluster_topics"
            | "analyze_findings"
            | "compare_results"
            | "compare_with_features"
            | "generate_report"
    );
    if !default_enabled {
        return None;
    }
    let slug = node
        .node_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        return None;
    }
    Some(format!(".tandem/artifacts/{slug}.{extension}"))
}

fn automation_node_allows_preexisting_output_reuse(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("allow_preexisting_output_reuse"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_web_research_expected(node: &AutomationFlowNode) -> bool {
    enforcement_requires_external_sources(&automation_node_output_enforcement(node))
}

pub(crate) fn automation_node_required_tools(node: &AutomationFlowNode) -> Vec<String> {
    automation_node_output_enforcement(node).required_tools
}

pub(crate) fn automation_node_execution_policy(
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> Value {
    let output_path = automation_node_required_output_path(node);
    let code_workflow = automation_node_is_code_workflow(node);
    let git_backed = workspace_has_git_repo(workspace_root);
    let mode = automation_node_execution_mode(node, workspace_root);
    let workflow_class = automation_node_workflow_class(node);
    json!({
        "mode": mode,
        "workflow_class": workflow_class,
        "code_workflow": code_workflow,
        "git_backed": git_backed,
        "declared_output_path": output_path,
        "project_backlog_tasks": automation_node_projects_backlog_tasks(node),
        "task_id": automation_node_task_id(node),
        "task_kind": automation_node_task_kind(node),
        "repo_root": automation_node_repo_root(node),
        "write_scope": automation_node_write_scope(node),
        "acceptance_criteria": automation_node_acceptance_criteria(node),
        "task_dependencies": automation_node_task_dependencies(node),
        "verification_state": automation_node_verification_state(node),
        "task_owner": automation_node_task_owner(node),
        "verification_command": automation_node_verification_command(node),
    })
}

fn resolve_automation_output_path(
    workspace_root: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let trimmed = output_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("required output path is empty");
    }
    let workspace = PathBuf::from(workspace_root);
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    if !resolved.starts_with(&workspace) {
        anyhow::bail!(
            "required output path `{}` must stay inside workspace `{}`",
            trimmed,
            workspace_root
        );
    }
    Ok(resolved)
}

fn automation_run_artifact_root(run_id: &str) -> Option<String> {
    let trimmed = run_id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(format!(".tandem/runs/{trimmed}/artifacts"))
    }
}

pub(crate) fn automation_run_scoped_output_path(run_id: &str, output_path: &str) -> Option<String> {
    let trimmed = output_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.replace('\\', "/");
    let prefix = ".tandem/artifacts/";
    if let Some(suffix) = normalized.strip_prefix(prefix) {
        let root = automation_run_artifact_root(run_id)?;
        return Some(if suffix.is_empty() {
            root
        } else {
            format!("{root}/{suffix}")
        });
    }
    Some(trimmed.to_string())
}

fn resolve_automation_output_path_for_run(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let scoped_output_path = automation_run_scoped_output_path(run_id, output_path)
        .unwrap_or_else(|| output_path.trim().to_string());
    resolve_automation_output_path(workspace_root, &scoped_output_path)
}

fn automation_node_output_sibling_extensions(node: &AutomationFlowNode) -> &'static [&'static str] {
    let kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim())
        .unwrap_or("structured_json");
    if kind.eq_ignore_ascii_case("report_markdown") {
        &["html", "htm", "md", "markdown", "txt"]
    } else {
        &[]
    }
}

pub(crate) fn automation_output_path_candidates(
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
) -> anyhow::Result<Vec<PathBuf>> {
    let resolved = resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    let mut candidates = vec![resolved.clone()];
    let sibling_extensions = automation_node_output_sibling_extensions(node);
    if sibling_extensions.is_empty() {
        return Ok(candidates);
    }

    let Some(parent) = resolved.parent() else {
        return Ok(candidates);
    };
    let Some(stem) = resolved.file_stem().and_then(|value| value.to_str()) else {
        return Ok(candidates);
    };

    let Ok(entries) = std::fs::read_dir(parent) else {
        return Ok(candidates);
    };
    let mut siblings = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && *path != resolved)
        .filter(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|candidate_stem| candidate_stem == stem)
        })
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| {
                    sibling_extensions
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
                })
        })
        .collect::<Vec<_>>();
    siblings.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    siblings.dedup();
    candidates.extend(siblings);
    candidates.dedup();
    Ok(candidates)
}

fn session_write_paths_for_output_candidates(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    candidate_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let candidate_paths = candidate_paths.iter().cloned().collect::<HashSet<_>>();
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let Ok(candidate_path) =
                resolve_automation_output_path_for_run(workspace_root, run_id, path)
            else {
                continue;
            };
            if !candidate_paths.contains(&candidate_path) {
                continue;
            }
            if !paths.iter().any(|existing| existing == &candidate_path) {
                paths.push(candidate_path);
            }
        }
    }
    paths
}

pub(crate) fn automation_resolve_verified_output_path(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let candidates = automation_output_path_candidates(workspace_root, run_id, node, output_path)?;
    let session_written_candidates =
        session_write_paths_for_output_candidates(session, workspace_root, run_id, &candidates);
    Ok(session_written_candidates
        .into_iter()
        .chain(candidates.into_iter())
        .find(|candidate| candidate.exists() && candidate.is_file()))
}

fn is_suspicious_automation_marker_file(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lowered = name.to_ascii_lowercase();
    lowered.starts_with(".tandem")
        || lowered == "_automation_touch.txt"
        || lowered.contains("stage-touch")
        || lowered.ends_with("-status.txt")
        || lowered.contains("touch.txt")
}

fn list_suspicious_automation_marker_files(workspace_root: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_suspicious_automation_marker_file(path))
        .filter_map(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn remove_suspicious_automation_marker_files(workspace_root: &str) {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_suspicious_automation_marker_file(&path) {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

pub(crate) fn automation_workspace_root_file_snapshot(
    workspace_root: &str,
) -> std::collections::BTreeSet<String> {
    let workspace = PathBuf::from(workspace_root);
    let mut snapshot = std::collections::BTreeSet::new();
    let mut stack = vec![workspace.clone()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let display = path
                .strip_prefix(&workspace)
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            snapshot.insert(display);
        }
    }
    snapshot
}

pub(crate) fn placeholder_like_artifact_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    // TODO(coding-hardening): Replace this phrase-based placeholder detection with
    // structural artifact validation. The long-term design should score artifact
    // substance from session mutation history + contract-kind-specific structure
    // (sections, length, citations, required headings) rather than hard-coded text
    // markers that are brittle across providers, prompts, and languages.
    if trimmed.len() <= 160 {
        let compact = trimmed.to_ascii_lowercase();
        let status_only_markers = [
            "completed",
            "written to",
            "already written",
            "no content change",
            "no content changes",
            "confirmed",
            "preserving existing artifact",
            "finalization",
            "write completion",
        ];
        if status_only_markers
            .iter()
            .any(|marker| compact.contains(marker))
            && !compact.contains("## ")
            && !compact.contains("\n## ")
            && !compact.contains("files reviewed")
            && !compact.contains("proof points")
        {
            return true;
        }
    }
    let lowered = trimmed
        .chars()
        .take(800)
        .collect::<String>()
        .to_ascii_lowercase();
    let strong_markers = [
        "completed previously in this run",
        "preserving file creation requirement",
        "preserving current workspace output state",
        "created/updated to satisfy workflow artifact requirement",
        "see existing workspace research already completed in this run",
        "already written in prior step",
        "no content changes needed",
        "placeholder preservation note",
        "touch file",
        "status note",
        "marker file",
    ];
    if strong_markers.iter().any(|marker| lowered.contains(marker)) {
        return true;
    }
    let status_markers = [
        "# status",
        "## status",
        "status: blocked",
        "status: completed",
        "status: pending",
        "blocked handoff",
        "blocked note",
        "not approved yet",
        "pending approval",
    ];
    status_markers.iter().any(|marker| lowered.contains(marker)) && trimmed.len() < 280
}

fn html_tag_count(text: &str, tag: &str) -> usize {
    let needle = format!("<{tag}");
    text.match_indices(&needle)
        .filter(|(index, _)| {
            let tail = &text[index + needle.len()..];
            tail.chars()
                .next()
                .is_none_or(|next| !next.is_ascii_alphabetic())
        })
        .count()
}

fn markdown_heading_count(text: &str) -> usize {
    let markdown = text
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .count();
    let html = (1..=6)
        .map(|level| html_tag_count(text, &format!("h{level}")))
        .sum::<usize>();
    markdown + html
}

fn markdown_list_item_count(text: &str) -> usize {
    let markdown = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit() && trimmed.contains('.'))
        })
        .count();
    markdown + html_tag_count(text, "li")
}

fn paragraph_block_count(text: &str) -> usize {
    let markdown = text
        .split("\n\n")
        .filter(|block| {
            let trimmed = block.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .count();
    markdown + html_tag_count(text, "p")
}

fn structural_substantive_artifact_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 180 {
        return false;
    }
    let heading_count = markdown_heading_count(trimmed);
    let list_count = markdown_list_item_count(trimmed);
    let paragraph_count = paragraph_block_count(trimmed);
    heading_count >= 2
        || (heading_count >= 1 && paragraph_count >= 3)
        || (paragraph_count >= 4)
        || (list_count >= 5)
}

fn substantive_artifact_text(text: &str) -> bool {
    structural_substantive_artifact_text(text)
}

fn artifact_required_section_count(node: &AutomationFlowNode, text: &str) -> usize {
    let lowered = text.to_ascii_lowercase();
    let headings = if automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
    {
        vec![
            "workspace source audit",
            "campaign goal",
            "target audience",
            "core pain points",
            "positioning angle",
            "competitor context",
            "proof points",
            "likely objections",
            "channel considerations",
            "recommended message hierarchy",
            "files reviewed",
            "files not reviewed",
            "web sources reviewed",
        ]
    } else {
        vec![
            "files reviewed",
            "review notes",
            "approved",
            "draft",
            "summary",
        ]
    };
    headings
        .iter()
        .filter(|heading| lowered.contains(**heading))
        .count()
}

pub(crate) fn normalize_workspace_display_path(
    workspace_root: &str,
    raw_path: &str,
) -> Option<String> {
    let trimmed = raw_path.trim().trim_matches('`');
    if trimmed.is_empty() {
        return None;
    }
    resolve_automation_output_path(workspace_root, trimmed)
        .ok()
        .and_then(|resolved| {
            resolved
                .strip_prefix(PathBuf::from(workspace_root))
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
        })
        .filter(|value| !value.is_empty())
}

fn tool_args_object(args: &Value) -> Option<std::borrow::Cow<'_, serde_json::Map<String, Value>>> {
    match args {
        Value::Object(map) => Some(std::borrow::Cow::Borrowed(map)),
        Value::String(raw) => {
            serde_json::from_str::<Value>(raw)
                .ok()
                .and_then(|value| match value {
                    Value::Object(map) => Some(std::borrow::Cow::Owned(map)),
                    _ => None,
                })
        }
        _ => None,
    }
}

pub(crate) fn session_read_paths(session: &Session, workspace_root: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("read")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str) else {
                continue;
            };
            if let Some(normalized) = normalize_workspace_display_path(workspace_root, path) {
                paths.push(normalized);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AutomationUpstreamEvidence {
    pub(crate) read_paths: Vec<String>,
    pub(crate) discovered_relevant_paths: Vec<String>,
    pub(crate) web_research_attempted: bool,
    pub(crate) web_research_succeeded: bool,
    pub(crate) citation_count: usize,
    pub(crate) citations: Vec<String>,
}

async fn collect_automation_upstream_research_evidence(
    state: &AppState,
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> AutomationUpstreamEvidence {
    let mut evidence = AutomationUpstreamEvidence::default();
    let mut upstream_node_ids = node
        .input_refs
        .iter()
        .map(|input| input.from_step_id.clone())
        .collect::<Vec<_>>();
    upstream_node_ids.extend(node.depends_on.clone());
    upstream_node_ids.sort();
    upstream_node_ids.dedup();
    let flow_nodes = automation
        .flow
        .nodes
        .iter()
        .map(|entry| (entry.node_id.as_str(), entry))
        .collect::<std::collections::HashMap<_, _>>();
    for upstream_node_id in upstream_node_ids {
        let Some(output) = run.checkpoint.node_outputs.get(&upstream_node_id) else {
            continue;
        };
        if let Some(validation) = output.get("artifact_validation") {
            if let Some(rows) = validation.get("read_paths").and_then(Value::as_array) {
                evidence
                    .read_paths
                    .extend(rows.iter().filter_map(Value::as_str).map(str::to_string));
            }
            if let Some(rows) = validation
                .get("discovered_relevant_paths")
                .and_then(Value::as_array)
            {
                evidence
                    .discovered_relevant_paths
                    .extend(rows.iter().filter_map(Value::as_str).map(str::to_string));
            }
            evidence.web_research_attempted |= validation
                .get("web_research_attempted")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            evidence.web_research_succeeded |= validation
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if let Some(count) = validation.get("citation_count").and_then(Value::as_u64) {
                evidence.citation_count += count as usize;
            }
            if let Some(rows) = validation.get("citations").and_then(Value::as_array) {
                evidence
                    .citations
                    .extend(rows.iter().filter_map(Value::as_str).map(str::to_string));
            }
        }
        if let Some(tool_telemetry) = output.get("tool_telemetry") {
            evidence.web_research_attempted |= tool_telemetry
                .get("web_research_used")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            evidence.web_research_succeeded |= tool_telemetry
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        if let Some(session_id) = automation_output_session_id(output) {
            if let Some(session) = state.storage.get_session(&session_id).await {
                evidence
                    .read_paths
                    .extend(session_read_paths(&session, workspace_root));
                evidence
                    .discovered_relevant_paths
                    .extend(session_discovered_relevant_paths(&session, workspace_root));
                if let Some(upstream_node) = flow_nodes.get(upstream_node_id.as_str()) {
                    let requested_tools = output
                        .get("tool_telemetry")
                        .and_then(|value| value.get("requested_tools"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let telemetry = summarize_automation_tool_activity(
                        upstream_node,
                        &session,
                        &requested_tools,
                    );
                    evidence.web_research_attempted |= telemetry
                        .get("web_research_used")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    evidence.web_research_succeeded |= telemetry
                        .get("web_research_succeeded")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                }
            }
        }
    }
    evidence.read_paths.sort();
    evidence.read_paths.dedup();
    evidence.discovered_relevant_paths.sort();
    evidence.discovered_relevant_paths.dedup();
    evidence.citations.sort();
    evidence.citations.dedup();
    evidence
}

fn session_discovered_relevant_paths(session: &Session, workspace_root: &str) -> Vec<String> {
    let workspace = PathBuf::from(workspace_root);
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                result,
                error,
                ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("glob")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(output) = automation_tool_result_output_text(result.as_ref()) else {
                continue;
            };
            for line in output.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let path = PathBuf::from(trimmed);
                let resolved = if path.is_absolute() {
                    path
                } else {
                    let Ok(resolved) = resolve_automation_output_path(workspace_root, trimmed)
                    else {
                        continue;
                    };
                    resolved
                };
                if !resolved.starts_with(&workspace) {
                    continue;
                }
                if !std::fs::metadata(&resolved)
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    continue;
                }
                let display = resolved
                    .strip_prefix(&workspace)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty());
                if let Some(display) = display {
                    paths.push(display);
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn session_write_candidates_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
) -> Vec<String> {
    let target_path = run_id
        .and_then(|run_id| {
            resolve_automation_output_path_for_run(workspace_root, run_id, declared_output_path)
                .ok()
        })
        .or_else(|| resolve_automation_output_path(workspace_root, declared_output_path).ok());
    let Some(target_path) = target_path else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let Ok(candidate_path) = (if let Some(run_id) = run_id {
                resolve_automation_output_path_for_run(workspace_root, run_id, path)
            } else {
                resolve_automation_output_path(workspace_root, path)
            }) else {
                continue;
            };
            if candidate_path != target_path {
                continue;
            }
            let Some(content) = args.get("content").and_then(Value::as_str) else {
                continue;
            };
            if !content.trim().is_empty() {
                candidates.push(content.to_string());
            }
        }
    }
    candidates
}

pub(crate) fn session_write_touched_output_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
) -> bool {
    let target_path = run_id
        .and_then(|run_id| {
            resolve_automation_output_path_for_run(workspace_root, run_id, declared_output_path)
                .ok()
        })
        .or_else(|| resolve_automation_output_path(workspace_root, declared_output_path).ok());
    let Some(target_path) = target_path else {
        return false;
    };
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let path = args
                .get("path")
                .or_else(|| args.get("file_path"))
                .or_else(|| args.get("filepath"))
                .or_else(|| args.get("output_path"))
                .or_else(|| args.get("target_path"))
                .or_else(|| args.get("file"))
                .and_then(Value::as_str)
                .map(str::trim);
            let Some(path) = path else {
                continue;
            };
            let Ok(candidate_path) = (if let Some(run_id) = run_id {
                resolve_automation_output_path_for_run(workspace_root, run_id, path)
            } else {
                resolve_automation_output_path(workspace_root, path)
            }) else {
                continue;
            };
            if candidate_path == target_path {
                return true;
            }
        }
    }
    false
}

pub(crate) fn session_file_mutation_summary(session: &Session, workspace_root: &str) -> Value {
    let mut touched_files = Vec::<String>::new();
    let mut mutation_tool_by_file = serde_json::Map::new();
    let workspace_root_path = PathBuf::from(workspace_root);
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                continue;
            }
            let tool_name = tool.trim().to_ascii_lowercase().replace('-', "_");
            let parsed_args = tool_args_object(args);
            let candidate_paths = if tool_name == "apply_patch" {
                parsed_args
                    .as_ref()
                    .and_then(|args| args.get("patchText"))
                    .and_then(Value::as_str)
                    .map(|patch| {
                        patch
                            .lines()
                            .filter_map(|line| {
                                let trimmed = line.trim();
                                trimmed
                                    .strip_prefix("*** Add File: ")
                                    .or_else(|| trimmed.strip_prefix("*** Update File: "))
                                    .or_else(|| trimmed.strip_prefix("*** Delete File: "))
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                    .map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                parsed_args
                    .as_ref()
                    .and_then(|args| args.get("path"))
                    .and_then(Value::as_str)
                    .map(|value| vec![value.trim().to_string()])
                    .unwrap_or_default()
            };
            for candidate in candidate_paths {
                let Some(resolved) = resolve_automation_output_path(workspace_root, &candidate)
                    .ok()
                    .or_else(|| {
                        let path = PathBuf::from(candidate.trim());
                        if path.is_absolute()
                            && tandem_core::is_within_workspace_root(&path, &workspace_root_path)
                        {
                            Some(path)
                        } else {
                            None
                        }
                    })
                else {
                    continue;
                };
                let display = resolved
                    .strip_prefix(&workspace_root_path)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| resolved.to_string_lossy().to_string());
                if !touched_files.iter().any(|existing| existing == &display) {
                    touched_files.push(display.clone());
                }
                match mutation_tool_by_file.get_mut(&display) {
                    Some(Value::Array(values)) => {
                        if !values
                            .iter()
                            .any(|value| value.as_str() == Some(tool_name.as_str()))
                        {
                            values.push(json!(tool_name.clone()));
                        }
                    }
                    _ => {
                        mutation_tool_by_file.insert(display.clone(), json!([tool_name.clone()]));
                    }
                }
            }
        }
    }
    touched_files.sort();
    json!({
        "touched_files": touched_files,
        "mutation_tool_by_file": mutation_tool_by_file,
    })
}

fn git_diff_summary_for_paths(workspace_root: &str, paths: &[String]) -> Option<Value> {
    if paths.is_empty() || !workspace_has_git_repo(workspace_root) {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(workspace_root)
        .arg("diff")
        .arg("--stat")
        .arg("--");
    for path in paths {
        cmd.arg(path);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if summary.is_empty() {
        None
    } else {
        Some(json!({
            "stat": summary
        }))
    }
}

#[cfg(test)]
pub(crate) fn validate_automation_artifact_output(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
) -> (Option<(String, String)>, Value, Option<String>) {
    validate_automation_artifact_output_with_upstream(
        node,
        session,
        workspace_root,
        None,
        session_text,
        tool_telemetry,
        preexisting_output,
        verified_output,
        workspace_snapshot_before,
        None,
    )
}

pub(crate) fn validate_automation_artifact_output_with_upstream(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    run_id: Option<&str>,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
    upstream_evidence: Option<&AutomationUpstreamEvidence>,
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
    let mut accepted_output = verified_output;
    let mut recovered_from_session_write = false;
    let current_read_paths = session_read_paths(session, workspace_root);
    let current_discovered_relevant_paths =
        session_discovered_relevant_paths(session, workspace_root);
    let use_upstream_evidence = automation_node_uses_upstream_validation_evidence(node);
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
    read_paths.sort();
    read_paths.dedup();
    discovered_relevant_paths.sort();
    discovered_relevant_paths.dedup();
    let mut reviewed_paths_backed_by_read = Vec::<String>::new();
    let mut unreviewed_relevant_paths = Vec::<String>::new();
    let mut unmet_requirements = Vec::<String>::new();
    let mut repair_attempted = false;
    let mut repair_succeeded = false;
    let mut citation_count = 0usize;
    let mut web_sources_reviewed_present = false;
    let mut heading_count = 0usize;
    let mut paragraph_count = 0usize;
    let mut artifact_candidates = Vec::<Value>::new();
    let mut accepted_candidate_source = None::<String>;
    let mut blocked_handoff_cleanup_action = None::<String>;
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
    let structured_handoff = if handoff_only_structured_json {
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
        let session_write_candidates =
            session_write_candidates_for_output(session, workspace_root, &path, run_id);
        let session_write_touched_required_output =
            session_write_touched_output_for_output(session, workspace_root, &path, run_id);
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
        let workspace_inspection_satisfied = tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || executed_has_read
            || (use_upstream_evidence && !discovered_relevant_paths.is_empty());
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
        let current_attempt_has_recorded_activity = !executed_tools_for_attempt.is_empty()
            || !session_write_candidates.is_empty()
            || (use_upstream_evidence && upstream_evidence.is_some());
        let preexisting_output_reuse_allowed =
            automation_node_allows_preexisting_output_reuse(node);
        let missing_current_attempt_output_write = requires_current_attempt_output
            && session_write_candidates.is_empty()
            && !session_write_touched_required_output
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
            rejected_reason = Some(format!(
                "required output `{}` was not created in the current attempt",
                path
            ));
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
            accepted_candidate_source = Some(best.source.clone());
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
        let required_tools_for_node = enforcement.required_tools.clone();
        let has_required_tools = !required_tools_for_node.is_empty();
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
        let has_research_contract = requires_local_source_reads
            || requires_external_sources
            || requires_files_reviewed
            || requires_files_not_reviewed
            || requires_citations
            || requires_web_sources_reviewed;
        let requires_read = required_tools_for_node.iter().any(|tool| tool == "read");
        let requires_websearch = required_tools_for_node
            .iter()
            .any(|tool| tool == "websearch")
            && !web_research_unavailable;
        if has_research_contract && (requested_has_read || requires_local_source_reads) {
            let missing_concrete_reads = requires_local_source_reads && !executed_has_read;
            let files_reviewed_backed = selected_assessment.is_some_and(|assessment| {
                !assessment.reviewed_paths.is_empty()
                    && assessment.reviewed_paths.len()
                        == assessment.reviewed_paths_backed_by_read.len()
            });
            let missing_file_coverage = (requires_files_reviewed
                && !selected_assessment
                    .is_some_and(|assessment| assessment.files_reviewed_present))
                || !files_reviewed_backed
                || (requires_files_not_reviewed && !unreviewed_relevant_paths.is_empty());
            let missing_web_research = requires_external_sources && !web_research_succeeded;
            let upstream_has_citations =
                use_upstream_evidence && upstream_evidence.is_some_and(|e| e.citation_count > 0);
            let missing_citations = requires_citations
                && !selected_assessment.is_some_and(|assessment| assessment.citation_count > 0)
                && !upstream_has_citations;
            let upstream_web_sources_reviewed = use_upstream_evidence
                && upstream_evidence.is_some_and(|e| e.web_research_succeeded);
            let missing_web_sources_reviewed = requires_web_sources_reviewed
                && !selected_assessment
                    .is_some_and(|assessment| assessment.web_sources_reviewed_present)
                && !upstream_web_sources_reviewed;
            let preserve_current_attempt_output_missing = unmet_requirements
                .iter()
                .any(|value| value == "current_attempt_output_missing");
            unmet_requirements.clear();
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
                || missing_citations
                || missing_file_coverage
                || missing_web_sources_reviewed
                || missing_web_research
                || has_path_hygiene_failure
            {
                semantic_block_reason = Some(if has_path_hygiene_failure {
                    "research artifact contains non-concrete paths (wildcards or directory placeholders) in source audit"
                        .to_string()
                } else if missing_concrete_reads {
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
            let missing_concrete_reads = requires_read && !executed_has_read;
            let missing_web_research =
                requires_websearch && requires_external_sources && !web_research_succeeded;
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            if missing_concrete_reads || missing_web_research {
                semantic_block_reason =
                    Some("artifact finalized without using required tools".to_string());
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
            let missing_editorial_substance =
                matches!(contract_kind.as_str(), "report_markdown" | "text_summary")
                    && !selected.as_ref().is_some_and(|assessment| {
                        !assessment.placeholder_like
                            && (assessment.substantive
                                || (assessment.length >= 120 && assessment.paragraph_count >= 1))
                    });
            let missing_markdown_structure = contract_kind == "report_markdown"
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
            if missing_editorial_substance {
                unmet_requirements.push("editorial_substance_missing".to_string());
            }
            if missing_markdown_structure {
                unmet_requirements.push("markdown_structure_missing".to_string());
            }
            if missing_upstream_synthesis {
                unmet_requirements.push("upstream_evidence_not_synthesized".to_string());
            }
            if semantic_block_reason.is_none()
                && (missing_editorial_substance
                    || missing_markdown_structure
                    || missing_upstream_synthesis)
            {
                semantic_block_reason = Some(if missing_upstream_synthesis {
                    "final artifact does not adequately synthesize the available upstream evidence"
                        .to_string()
                } else if missing_markdown_structure {
                    "editorial artifact is missing expected markdown structure".to_string()
                } else {
                    "editorial artifact is too weak or placeholder-like".to_string()
                });
            }
        }
        let writes_blocked_handoff_artifact = accepted_output
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
        {
            accepted_output = None;
        }
    }
    if accepted_output.is_some() && accepted_candidate_source.is_none() {
        accepted_candidate_source = Some("verified_output".to_string());
    }
    if handoff_only_structured_json {
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
        let requested_has_websearch = requested_tools
            .iter()
            .any(|value| value.as_str() == Some("websearch"));
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
        let requires_read = enforcement.required_tools.iter().any(|tool| tool == "read");
        let requires_websearch = enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "websearch")
            && !web_research_unavailable;
        let requires_workspace_inspection = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "workspace_inspection");
        let requires_concrete_reads = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "concrete_reads");
        let requires_successful_web_research = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research")
            && !web_research_unavailable;

        if structured_handoff.is_none() {
            unmet_requirements.push("structured_handoff_missing".to_string());
        }
        if requires_workspace_inspection && !workspace_inspection_satisfied {
            unmet_requirements.push("workspace_inspection_required".to_string());
        }
        if (requires_read || requires_concrete_reads) && !executed_has_read {
            unmet_requirements.push("no_concrete_reads".to_string());
        }
        if requires_concrete_reads && !executed_has_read {
            unmet_requirements.push("concrete_read_required".to_string());
        }
        if (requires_websearch || requires_successful_web_research) && !web_research_succeeded {
            unmet_requirements.push("missing_successful_web_research".to_string());
        }
        unmet_requirements.sort();
        unmet_requirements.dedup();
    }
    let validation_profile = enforcement
        .validation_profile
        .clone()
        .unwrap_or_else(|| "artifact_only".to_string());
    unmet_requirements.sort();
    unmet_requirements.dedup();
    let mut warning_requirements = unmet_requirements
        .iter()
        .filter(|item| validation_requirement_is_warning(&validation_profile, item))
        .cloned()
        .collect::<Vec<_>>();
    unmet_requirements.retain(|item| !validation_requirement_is_warning(&validation_profile, item));
    warning_requirements.sort();
    warning_requirements.dedup();
    semantic_block_reason = semantic_block_reason_for_requirements(&unmet_requirements);
    let (repair_attempt, repair_attempts_remaining, repair_exhausted) = infer_artifact_repair_state(
        parsed_status.as_ref(),
        repair_attempted,
        repair_succeeded,
        semantic_block_reason.as_deref(),
        tool_telemetry,
    );
    let has_required_tools = !enforcement.required_tools.is_empty();
    let contract_requires_repair = !enforcement.retry_on_missing.is_empty()
        || has_required_tools
        || handoff_only_structured_json;
    let validation_outcome = if contract_requires_repair && semantic_block_reason.is_some() {
        if repair_exhausted {
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
    let required_next_tool_actions = if should_classify {
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
            &unreviewed_relevant_paths,
            latest_web_research_failure,
        )
    } else {
        Vec::new()
    };

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
        "verification": verification_summary,
        "git_diff_summary": git_diff_summary_for_paths(workspace_root, &touched_files),
        "read_paths": read_paths,
        "current_node_read_paths": current_read_paths,
        "discovered_relevant_paths": discovered_relevant_paths,
        "current_node_discovered_relevant_paths": current_discovered_relevant_paths,
        "reviewed_paths_backed_by_read": reviewed_paths_backed_by_read,
        "unreviewed_relevant_paths": unreviewed_relevant_paths,
        "citation_count": if use_upstream_evidence {
            json!(citation_count.saturating_add(
                upstream_evidence.map(|e| e.citation_count).unwrap_or(0)
            ))
        } else {
            json!(citation_count)
        },
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
        "blocked_handoff_cleanup_action": blocked_handoff_cleanup_action,
        "repair_attempted": repair_attempted,
        "repair_attempt": repair_attempt,
        "repair_attempts_remaining": repair_attempts_remaining,
        "repair_budget_spent": repair_attempt > 0,
        "repair_succeeded": repair_succeeded,
        "repair_exhausted": repair_exhausted,
        "validation_outcome": validation_outcome,
        "validation_profile": validation_profile,
        "blocking_classification": blocking_classification,
        "required_next_tool_actions": required_next_tool_actions,
        "unmet_requirements": unmet_requirements,
        "warning_requirements": warning_requirements.clone(),
        "warning_count": warning_requirements.len(),
        "artifact_candidates": artifact_candidates,
        "resolved_enforcement": enforcement,
        "structured_handoff_present": structured_handoff.is_some(),
    });
    let rejected = metadata
        .get("rejected_artifact_reason")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            metadata
                .get("semantic_block_reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    (accepted_output, metadata, rejected)
}

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
                let metadata = automation_tool_result_metadata(result.as_ref())
                    .cloned()
                    .unwrap_or(Value::Null);
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
                    || automation_tool_result_output_payload(result.as_ref()).is_some_and(
                        |payload| {
                            payload
                                .get("result_count")
                                .and_then(Value::as_u64)
                                .is_some_and(|count| count > 0)
                                || payload
                                    .get("results")
                                    .and_then(Value::as_array)
                                    .is_some_and(|results| !results.is_empty())
                        },
                    );
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
                if (result_error.is_none() || result_has_sources)
                    && !timed_out
                    && !unavailable
                    && !output.is_empty()
                {
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

async fn load_automation_session_after_run(
    state: &AppState,
    session_id: &str,
    expect_tool_activity: bool,
) -> Option<Session> {
    let mut last = state.storage.get_session(session_id).await?;
    if !expect_tool_activity || session_contains_tool_invocations(&last) {
        return Some(last);
    }

    // `message.part.updated` events are persisted asynchronously; give storage a
    // short window to capture tool invocations before we compute telemetry.
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        let current = state.storage.get_session(session_id).await?;
        if session_contains_tool_invocations(&current) {
            return Some(current);
        }
        last = current;
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
        let idempotency_key = crate::sha256_hex(&[
            "automation_v2",
            &automation.automation_id,
            run_id,
            &node.node_id,
            &attempt.to_string(),
            tool,
            &args.to_string(),
            &call_index.to_string(),
        ]);
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

fn automation_node_is_outbound_action(node: &AutomationFlowNode) -> bool {
    if node
        .metadata
        .as_ref()
        .and_then(|value| value.pointer("/builder/role"))
        .and_then(Value::as_str)
        .is_some_and(|role| role.eq_ignore_ascii_case("publisher"))
    {
        return true;
    }
    let objective = node.objective.to_ascii_lowercase();
    [
        "publish", "post ", "send ", "notify", "deliver", "submit", "share",
    ]
    .iter()
    .any(|needle| objective.contains(needle))
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
    if automation_output_validator_kind(node) == crate::AutomationOutputValidatorKind::ResearchBrief
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

async fn resolve_automation_v2_workspace_root(
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

fn automation_declared_output_paths_for_run(
    automation: &AutomationV2Spec,
    run_id: &str,
) -> Vec<String> {
    automation_declared_output_paths(automation)
        .into_iter()
        .filter_map(|path| automation_run_scoped_output_path(run_id, &path))
        .collect::<Vec<_>>()
}

pub(crate) async fn clear_automation_declared_outputs(
    state: &AppState,
    automation: &AutomationV2Spec,
    run_id: &str,
) -> anyhow::Result<()> {
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    for output_path in automation_declared_output_paths_for_run(automation, run_id) {
        if let Ok(resolved) = resolve_automation_output_path(&workspace_root, &output_path) {
            if resolved.exists() {
                let _ = std::fs::remove_file(&resolved);
            }
        }
    }
    remove_suspicious_automation_marker_files(&workspace_root);
    Ok(())
}

pub async fn clear_automation_subtree_outputs(
    state: &AppState,
    automation: &AutomationV2Spec,
    run_id: &str,
    node_ids: &std::collections::HashSet<String>,
) -> anyhow::Result<Vec<String>> {
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    let mut cleared = Vec::new();
    for node in &automation.flow.nodes {
        if !node_ids.contains(&node.node_id) {
            continue;
        }
        let Some(output_path) = automation_node_required_output_path(node) else {
            continue;
        };
        let candidates =
            automation_output_path_candidates(&workspace_root, run_id, node, &output_path)?;
        for resolved in candidates {
            if !resolved.exists() || !resolved.is_file() {
                continue;
            }
            std::fs::remove_file(&resolved).map_err(|error| {
                anyhow::anyhow!(
                    "failed to clear subtree output `{}` for automation `{}`: {}",
                    output_path,
                    automation.automation_id,
                    error
                )
            })?;
            if let Some(display) = resolved
                .strip_prefix(&workspace_root)
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
                .filter(|value| !value.is_empty())
            {
                cleared.push(display);
            } else {
                cleared.push(output_path.clone());
            }
        }
    }
    let had_markers = !list_suspicious_automation_marker_files(&workspace_root).is_empty();
    if had_markers {
        remove_suspicious_automation_marker_files(&workspace_root);
    }
    cleared.sort();
    cleared.dedup();
    Ok(cleared)
}

pub(crate) async fn execute_automation_v2_node(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    agent: &AutomationAgentProfile,
) -> anyhow::Result<Value> {
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{}` not found", run_id))?;
    let start_cost_usd = run.estimated_cost_usd;
    let start_prompt_tokens = run.prompt_tokens;
    let start_completion_tokens = run.completion_tokens;

    // Phase 5: Check PreexistingArtifactRegistry (MWF-300)
    let prevalidated = {
        let scheduler = state.automation_scheduler.read().await;
        if scheduler
            .preexisting_registry
            .is_artifact_prevalidated(run_id, &node.node_id)
        {
            let path = scheduler
                .preexisting_registry
                .get_prevalidated_path(run_id, &node.node_id)
                .map(|s| s.to_string());
            let digest = scheduler
                .preexisting_registry
                .artifacts
                .get(run_id)
                .and_then(|m| m.get(&node.node_id))
                .map(|a| a.content_digest.clone());
            Some((path, digest))
        } else {
            None
        }
    };

    if let Some((Some(output_path), Some(content_digest))) = prevalidated {
        let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
        let resolved =
            resolve_automation_output_path_for_run(&workspace_root, run_id, &output_path)?;
        if resolved.exists() {
            let current_content = std::fs::read_to_string(&resolved).ok();
            let current_digest = current_content.as_ref().map(|c| sha256_hex(&[c]));
            if current_digest.as_ref() == Some(&content_digest) {
                tracing::info!(
                    run_id = %run_id,
                    node_id = %node.node_id,
                    path = %output_path,
                    "reusing prevalidated artifact from registry (MWF-300)"
                );

                // Build a dummy session to satisfy the output wrapper
                let mut session = Session::new(
                    Some(format!(
                        "Automation {} / {} (Reused)",
                        automation.automation_id, node.node_id
                    )),
                    Some(workspace_root.clone()),
                );
                let session_id = session.id.clone();
                session.project_id = Some(automation_workspace_project_id(&workspace_root));
                session.workspace_root = Some(workspace_root.clone());
                session.messages.push(tandem_types::Message::new(
                    tandem_types::MessageRole::Assistant,
                    vec![tandem_types::MessagePart::Text {
                        text: format!(
                            "Reusing previously validated artifact `{}`.\n\n{{\"status\":\"completed\"}}",
                            output_path
                        ),
                    }],
                ));
                state.storage.save_session(session.clone()).await?;

                let output = node_output::wrap_automation_node_output(
                    node,
                    &session,
                    &[],
                    &session_id,
                    Some(run_id),
                    "Reusing previously validated artifact.",
                    Some((output_path, current_content.unwrap())),
                    Some(json!({
                        "accepted_candidate_source": "preexisting_output",
                        "status": "reused_valid"
                    })),
                );
                return Ok(output);
            }
        }
    }

    let attempt = run
        .checkpoint
        .node_attempts
        .get(&node.node_id)
        .copied()
        .unwrap_or(1);
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    let upstream_inputs = build_automation_v2_upstream_inputs(&run, node, &workspace_root)?;
    let workspace_path = PathBuf::from(&workspace_root);
    if !workspace_path.exists() {
        anyhow::bail!(
            "workspace_root `{}` for automation `{}` does not exist",
            workspace_root,
            automation.automation_id
        );
    }
    if !workspace_path.is_dir() {
        anyhow::bail!(
            "workspace_root `{}` for automation `{}` is not a directory",
            workspace_root,
            automation.automation_id
        );
    }
    let required_output_path = automation_node_required_output_path_for_run(node, Some(run_id));
    if let (Some(output_path), Some(payload)) = (
        required_output_path.as_deref(),
        automation_node_inline_artifact_payload(node),
    ) {
        let verified_output =
            write_automation_inline_artifact(&workspace_root, run_id, output_path, &payload)?;
        let mut session = Session::new(
            Some(format!(
                "Automation {} / {}",
                automation.automation_id, node.node_id
            )),
            Some(workspace_root.clone()),
        );
        let session_id = session.id.clone();
        session.project_id = Some(automation_workspace_project_id(&workspace_root));
        session.workspace_root = Some(workspace_root.clone());
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::Text {
                text: format!(
                    "Prepared deterministic workflow artifact `{}` from the node inputs.\n\n{{\"status\":\"completed\"}}",
                    output_path
                ),
            }],
        ));
        state.storage.save_session(session.clone()).await?;
        tracing::info!(
            run_id = %run_id,
            automation_id = %automation.automation_id,
            node_id = %node.node_id,
            output_path = %output_path,
            "automation node used deterministic inline artifact shortcut"
        );
        let output = node_output::wrap_automation_node_output(
            node,
            &session,
            &[],
            &session_id,
            Some(run_id),
            "Prepared deterministic workflow artifact from inline node inputs.",
            Some(verified_output),
            Some(json!({
                "deterministic_artifact": true,
                "deterministic_source": "node_metadata_inputs",
                "accepted_candidate_source": "verified_output",
                "unmet_requirements": [],
            })),
        );
        return Ok(output);
    }
    let template = if let Some(template_id) = agent.template_id.as_deref().map(str::trim) {
        if template_id.is_empty() {
            None
        } else {
            state
                .agent_teams
                .get_template_for_workspace(&workspace_root, template_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("agent template `{}` not found", template_id))
                .map(Some)?
        }
    } else {
        None
    };
    let mut session = Session::new(
        Some(format!(
            "Automation {} / {}",
            automation.automation_id, node.node_id
        )),
        Some(workspace_root.clone()),
    );
    let session_id = session.id.clone();
    let project_id = automation_workspace_project_id(&workspace_root);
    session.project_id = Some(project_id.clone());
    session.workspace_root = Some(workspace_root.clone());
    state.storage.save_session(session).await?;

    state.add_automation_v2_session(run_id, &session_id).await;

    let mut allowlist = merge_automation_agent_allowlist(agent, template.as_ref());
    if let Some(mcp_tools) = agent.mcp_policy.allowed_tools.as_ref() {
        allowlist.extend(mcp_tools.clone());
    }
    let mcp_tool_diagnostics = sync_automation_allowed_mcp_servers(
        state,
        node,
        &agent.mcp_policy.allowed_servers,
        &allowlist,
    )
    .await;
    let available_tool_schemas = state.tools.list().await;
    let available_tool_names = available_tool_schemas
        .iter()
        .map(|schema| schema.name.clone())
        .collect::<HashSet<_>>();
    let requested_tools = automation_requested_tools_for_node(
        node,
        &workspace_root,
        allowlist.clone(),
        &available_tool_names,
    );
    let mut requested_tools = requested_tools;
    if !requested_tools.iter().any(|tool| tool == "mcp_list") {
        requested_tools.push("mcp_list".to_string());
    }
    let effective_offered_tools =
        automation_expand_effective_offered_tools(&requested_tools, &available_tool_names);
    let execution_mode = automation_node_execution_mode(node, &workspace_root);
    let mut capability_resolution = automation_resolve_capabilities(
        node,
        execution_mode,
        &effective_offered_tools,
        &available_tool_names,
    );
    let selected_mcp_servers = mcp_tool_diagnostics
        .get("selected_servers")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty());
    if automation_node_requires_email_delivery(node) || selected_mcp_servers {
        automation_merge_mcp_capability_diagnostics(
            &mut capability_resolution,
            &mcp_tool_diagnostics,
        );
    }
    let offered_tool_schemas = available_tool_schemas
        .iter()
        .filter(|schema| {
            effective_offered_tools
                .iter()
                .any(|tool| tool == &schema.name)
        })
        .cloned()
        .collect::<Vec<_>>();
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, requested_tools.clone())
        .await;
    state
        .engine_loop
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    let model = resolve_automation_agent_model(agent, template.as_ref());
    let preexisting_output = required_output_path
        .as_deref()
        .and_then(|output_path| {
            automation_output_path_candidates(&workspace_root, run_id, node, output_path)
                .ok()
                .and_then(|candidates| {
                    candidates
                        .into_iter()
                        .find(|candidate| candidate.exists() && candidate.is_file())
                })
        })
        .and_then(|resolved| std::fs::read_to_string(resolved).ok());
    let workspace_snapshot_before = automation_workspace_root_file_snapshot(&workspace_root);
    let standup_report_path = if is_agent_standup_automation(automation)
        && node.node_id == "standup_synthesis"
    {
        resolve_standup_report_path_for_run(automation, run.started_at_ms.unwrap_or_else(now_ms))
    } else {
        None
    };
    let max_attempts = automation_node_max_attempts(node);
    let mut prompt = render_automation_v2_prompt(
        automation,
        &workspace_root,
        run_id,
        node,
        attempt,
        agent,
        &upstream_inputs,
        &requested_tools,
        template
            .as_ref()
            .and_then(|value| value.system_prompt.as_deref()),
        standup_report_path.as_deref(),
        if is_agent_standup_automation(automation) {
            Some(project_id.as_str())
        } else {
            None
        },
    );
    let mut preflight = build_automation_prompt_preflight(
        &prompt,
        &effective_offered_tools,
        &offered_tool_schemas,
        execution_mode,
        &capability_resolution,
        "standard",
        false,
    );
    if automation_preflight_should_degrade(&preflight) && !upstream_inputs.is_empty() {
        prompt = render_automation_v2_prompt_with_options(
            automation,
            &workspace_root,
            run_id,
            node,
            attempt,
            agent,
            &upstream_inputs,
            &requested_tools,
            template
                .as_ref()
                .and_then(|value| value.system_prompt.as_deref()),
            standup_report_path.as_deref(),
            if is_agent_standup_automation(automation) {
                Some(project_id.as_str())
            } else {
                None
            },
            AutomationPromptRenderOptions {
                summary_only_upstream: true,
            },
        );
        preflight = build_automation_prompt_preflight(
            &prompt,
            &effective_offered_tools,
            &offered_tool_schemas,
            execution_mode,
            &capability_resolution,
            "summary_only_upstream",
            true,
        );
    }
    if let Some(repair_brief) = render_automation_repair_brief(
        node,
        run.checkpoint.node_outputs.get(&node.node_id),
        attempt,
        max_attempts,
    ) {
        prompt.push_str("\n\n");
        prompt.push_str(&repair_brief);
    }
    let req = SendMessageRequest {
        parts: vec![MessagePartInput::Text { text: prompt }],
        model: model.clone(),
        agent: None,
        tool_mode: Some(ToolMode::Required),
        tool_allowlist: Some(requested_tools.clone()),
        context_mode: None,
        write_required: required_output_path.as_ref().map(|_| true),
        prewrite_requirements: automation_node_prewrite_requirements(node, &requested_tools),
    };
    let result = state
        .engine_loop
        .run_prompt_async_with_context(
            session_id.clone(),
            req,
            Some(format!("automation-v2:{run_id}")),
        )
        .await;

    state
        .engine_loop
        .clear_session_allowed_tools(&session_id)
        .await;
    state
        .engine_loop
        .clear_session_auto_approve_permissions(&session_id)
        .await;
    state.clear_automation_v2_session(run_id, &session_id).await;

    result?;
    let expect_tool_activity = !requested_tools.is_empty();
    let session = load_automation_session_after_run(state, &session_id, expect_tool_activity)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation session `{}` missing after run", session_id))?;
    let session_text = extract_session_text_output(&session);
    let verified_output = if let Some(output_path) = required_output_path.as_deref() {
        let resolved = automation_resolve_verified_output_path(
            &session,
            &workspace_root,
            run_id,
            node,
            output_path,
        )?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "required output `{}` was not created for node `{}`",
                output_path,
                node.node_id
            )
        })?;
        if !resolved.is_file() {
            anyhow::bail!(
                "required output `{}` for node `{}` is not a file",
                output_path,
                node.node_id
            );
        }
        let file_text = std::fs::read_to_string(&resolved).map_err(|error| {
            anyhow::anyhow!(
                "required output `{}` for node `{}` could not be read: {}",
                output_path,
                node.node_id,
                error
            )
        })?;
        let display_path = resolved
            .strip_prefix(&workspace_root)
            .ok()
            .and_then(|value| value.to_str().map(str::to_string))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| output_path.to_string());
        Some((display_path, file_text))
    } else {
        None
    };
    let tool_telemetry = summarize_automation_tool_activity(node, &session, &requested_tools);
    let mut tool_telemetry = tool_telemetry;
    let base_attempt_evidence = node_output::build_automation_attempt_evidence(
        node,
        attempt,
        &session,
        &session_id,
        &workspace_root,
        &tool_telemetry,
        &preflight,
        &capability_resolution,
        verified_output.as_ref(),
    );
    if let Some(object) = tool_telemetry.as_object_mut() {
        object.insert("preflight".to_string(), preflight.clone());
        object.insert(
            "capability_resolution".to_string(),
            capability_resolution.clone(),
        );
        object.insert(
            "attempt_evidence".to_string(),
            base_attempt_evidence.clone(),
        );
    }
    let upstream_evidence = if automation_node_uses_upstream_validation_evidence(node) {
        Some(
            collect_automation_upstream_research_evidence(
                state,
                automation,
                &run,
                node,
                &workspace_root,
            )
            .await,
        )
    } else {
        None
    };
    let (verified_output, mut artifact_validation, artifact_rejected_reason) =
        validate_automation_artifact_output_with_upstream(
            node,
            &session,
            &workspace_root,
            Some(run_id.as_ref()),
            &session_text,
            &tool_telemetry,
            preexisting_output.as_deref(),
            verified_output,
            &workspace_snapshot_before,
            upstream_evidence.as_ref(),
        );
    let _ = artifact_rejected_reason;
    let editorial_publish_block_reason = state
        .get_automation_v2_run(run_id)
        .await
        .and_then(|run| automation_publish_editorial_block_reason(&run, node));
    if let Some(reason) = editorial_publish_block_reason.as_ref() {
        if let Some(object) = artifact_validation.as_object_mut() {
            let unmet = object
                .entry("unmet_requirements".to_string())
                .or_insert_with(|| json!([]));
            if let Some(rows) = unmet.as_array_mut() {
                if !rows
                    .iter()
                    .any(|value| value.as_str() == Some("editorial_clearance_required"))
                {
                    rows.push(json!("editorial_clearance_required"));
                }
            }
            object
                .entry("semantic_block_reason".to_string())
                .or_insert_with(|| Value::String(reason.clone()));
        }
    }
    let external_actions = if editorial_publish_block_reason.is_some() {
        Vec::new()
    } else {
        record_automation_external_actions_for_session(
            state,
            run_id,
            automation,
            node,
            attempt,
            &session_id,
            &session,
        )
        .await?
    };
    let mut output = wrap_automation_node_output(
        node,
        &session,
        &requested_tools,
        &session_id,
        Some(run_id),
        &session_text,
        verified_output,
        Some(artifact_validation),
    );
    let run_after = state.get_automation_v2_run(run_id).await.unwrap_or(run);
    let cost_usd_delta = run_after.estimated_cost_usd - start_cost_usd;
    let prompt_tokens_delta = run_after.prompt_tokens.saturating_sub(start_prompt_tokens);
    let completion_tokens_delta = run_after
        .completion_tokens
        .saturating_sub(start_completion_tokens);
    let budget_limit_reached = automation
        .execution
        .max_total_cost_usd
        .map(|max| run_after.estimated_cost_usd >= max)
        .unwrap_or(false);
    let cost_provenance = automation_step_cost_provenance(
        &node.node_id,
        model.map(|m| m.model_id.clone()),
        prompt_tokens_delta,
        completion_tokens_delta,
        cost_usd_delta,
        run_after.estimated_cost_usd,
        budget_limit_reached,
    );
    if let Some(object) = output.as_object_mut() {
        object.insert("cost_provenance".to_string(), cost_provenance);
        if !external_actions.is_empty() {
            object.insert(
                "external_actions".to_string(),
                serde_json::to_value(&external_actions)?,
            );
        }
    }
    Ok(output)
}

pub mod tasks;

pub async fn run_automation_v2_executor(state: AppState) {
    tasks::run_automation_v2_executor(state).await
}

#[cfg(test)]
mod tests;
