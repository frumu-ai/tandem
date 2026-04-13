use std::collections::HashSet;
use std::path::{Component, PathBuf};
use std::time::Duration;

pub(crate) mod assessment;
pub(crate) mod capability_impl;
pub(crate) mod enforcement;
pub(crate) mod extraction;
pub(crate) use extraction::{
    detect_glob_loop, extract_recoverable_json_artifact,
    extract_recoverable_json_artifact_prefer_standup, extract_session_text_output,
};
pub(crate) mod legacy_defaults;
pub(crate) mod lifecycle;
pub(crate) mod node_output;
pub(crate) mod node_runtime_impl;
pub(crate) mod path_hygiene;
pub(crate) mod prompting_impl;
pub(crate) mod rate_limit;
pub(crate) mod receipts;
pub(crate) mod scheduler;
pub(crate) mod types;
pub(crate) mod upstream;
pub(crate) mod validation;
pub(crate) mod verification;
mod workflow_impl;
use assessment::*;
pub(crate) use capability_impl::*;
use enforcement::*;
use extraction::*;
pub(crate) use legacy_defaults::{
    automation_node_allows_attachments, automation_node_builder_metadata,
    automation_node_builder_string_array, automation_node_delivery_method,
    automation_node_delivery_target, automation_node_email_content_type,
    automation_node_inline_body_only, automation_node_is_outbound_action,
    automation_node_is_research_finalize, automation_node_preserves_full_upstream_inputs,
    automation_node_requires_email_delivery, automation_node_uses_upstream_validation_evidence,
};
use lifecycle::*;
pub use lifecycle::{record_automation_lifecycle_event, record_automation_workflow_state_events};
pub(crate) use node_output::enrich_automation_node_output_for_contract;
pub(crate) use node_output::research_required_next_tool_actions;
use node_output::*;
use path_hygiene::*;
use receipts::*;
pub use scheduler::{
    AutomationScheduler, PreexistingArtifactRegistry, QueueReason, SchedulerMetadata,
    ValidatedArtifact,
};
use types::*;
use upstream::*;
use validation::*;
use verification::*;
pub(crate) use workflow_impl::migrate_bundled_studio_research_split_automation;

pub fn automation_node_output_enforcement(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputEnforcement {
    enforcement::automation_node_output_enforcement(node)
}

pub(crate) fn automation_node_research_stage(node: &AutomationFlowNode) -> Option<String> {
    legacy_defaults::automation_node_research_stage(node)
}

pub(crate) async fn resolve_automation_agent_template(
    state: &AppState,
    workspace_root: &str,
    template_id: &str,
) -> anyhow::Result<Option<tandem_orchestrator::AgentTemplate>> {
    let template_id = template_id.trim();
    if template_id.is_empty() {
        return Ok(None);
    }

    if let Some(template) = state
        .agent_teams
        .get_template_for_workspace(workspace_root, template_id)
        .await?
    {
        return Ok(Some(template));
    }

    let global_workspace_root = state.workspace_index.snapshot().await.root;
    if global_workspace_root == workspace_root {
        return Ok(None);
    }

    state
        .agent_teams
        .get_template_for_workspace(&global_workspace_root, template_id)
        .await
}

use serde_json::{json, Value};
use tandem_core::resolve_shared_paths;
use tandem_memory::MemoryManager;
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

#[derive(Clone, Debug, Default)]
pub(crate) struct AutomationPromptRuntimeValues {
    pub(crate) current_date: String,
    pub(crate) current_time: String,
    pub(crate) current_timestamp: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AutomationPromptRenderOptions {
    pub(crate) summary_only_upstream: bool,
    pub(crate) knowledge_context: Option<String>,
    pub(crate) runtime_values: Option<AutomationPromptRuntimeValues>,
}

fn automation_prompt_runtime_values(started_at_ms: Option<u64>) -> AutomationPromptRuntimeValues {
    let started_at_ms = started_at_ms.unwrap_or_else(now_ms);
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(started_at_ms as i64)
        .unwrap_or_else(chrono::Utc::now);
    AutomationPromptRuntimeValues {
        current_date: timestamp.format("%Y-%m-%d").to_string(),
        current_time: timestamp.format("%H%M").to_string(),
        current_timestamp: timestamp.format("%Y-%m-%d %H:%M").to_string(),
    }
}

fn automation_effective_knowledge_binding(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
) -> tandem_orchestrator::KnowledgeBinding {
    let default = tandem_orchestrator::KnowledgeBinding::default();
    let mut binding = automation.knowledge.clone();
    let overlay = &node.knowledge;

    if overlay.enabled != default.enabled {
        binding.enabled = overlay.enabled;
    }
    if overlay.reuse_mode != default.reuse_mode {
        binding.reuse_mode = overlay.reuse_mode;
    }
    if overlay.trust_floor != default.trust_floor {
        binding.trust_floor = overlay.trust_floor;
    }
    if !overlay.read_spaces.is_empty() {
        binding.read_spaces = overlay.read_spaces.clone();
    }
    if !overlay.promote_spaces.is_empty() {
        binding.promote_spaces = overlay.promote_spaces.clone();
    }
    if overlay.namespace.is_some() {
        binding.namespace = overlay.namespace.clone();
    }
    if overlay.subject.is_some() {
        binding.subject = overlay.subject.clone();
    }
    if overlay.freshness_ms.is_some() {
        binding.freshness_ms = overlay.freshness_ms;
    }

    binding
}

async fn automation_knowledge_preflight(
    state: &AppState,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    run_id: &str,
    project_id: &str,
) -> Option<tandem_orchestrator::KnowledgePreflightResult> {
    let binding = automation_effective_knowledge_binding(automation, node);
    if !binding.enabled || binding.reuse_mode == tandem_orchestrator::KnowledgeReuseMode::Disabled {
        return None;
    }
    let subject = binding
        .subject
        .clone()
        .unwrap_or_else(|| node.objective.trim().to_string());
    if subject.trim().is_empty() {
        return None;
    }
    let task_family = automation_node_knowledge_task_family(node);
    let paths = resolve_shared_paths().ok()?;
    let manager = MemoryManager::new(&paths.memory_db_path).await.ok()?;
    let preflight = manager
        .preflight_knowledge(&tandem_orchestrator::KnowledgePreflightRequest {
            project_id: project_id.to_string(),
            task_family: task_family.clone(),
            subject,
            binding,
        })
        .await
        .ok()?;
    if preflight.is_reusable() {
        state.event_bus.publish(EngineEvent::new(
            "knowledge.preflight.injected",
            json!({
                "automationID": automation.automation_id,
                "runID": run_id,
                "nodeID": node.node_id,
                "taskFamily": task_family,
                "decision": preflight.decision.to_string(),
                "coverageKey": preflight.coverage_key,
                "itemCount": preflight.items.len(),
            }),
        ));
    }
    Some(preflight)
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

fn automation_capability_resolution_missing_capabilities(
    capability_resolution: &Value,
) -> Vec<String> {
    capability_resolution
        .get("missing_capabilities")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn automation_reset_attempt_tool_failure_labels(tool_telemetry: &mut Value) {
    let Some(object) = tool_telemetry.as_object_mut() else {
        return;
    };
    object.insert("latest_web_research_failure".to_string(), Value::Null);
    object.insert("latest_email_delivery_failure".to_string(), Value::Null);
    if let Some(web_research) = object
        .get_mut("attempt_evidence")
        .and_then(Value::as_object_mut)
        .and_then(|value| value.get_mut("evidence"))
        .and_then(Value::as_object_mut)
        .and_then(|value| value.get_mut("web_research"))
        .and_then(Value::as_object_mut)
    {
        web_research.insert("latest_failure".to_string(), Value::Null);
    }
    if let Some(delivery) = object
        .get_mut("attempt_evidence")
        .and_then(Value::as_object_mut)
        .and_then(|value| value.get_mut("delivery"))
        .and_then(Value::as_object_mut)
    {
        delivery.insert("latest_failure".to_string(), Value::Null);
    }
}

fn automation_initialized_attempt_tool_telemetry(
    requested_tools: &[String],
    capability_resolution: &Value,
) -> Value {
    let mut tool_telemetry = json!({
        "requested_tools": requested_tools,
        "executed_tools": [],
        "tool_call_counts": {},
        "workspace_inspection_used": false,
        "web_research_used": false,
        "web_research_succeeded": false,
        "latest_web_research_failure": Value::Null,
        "email_delivery_attempted": false,
        "email_delivery_succeeded": false,
        "latest_email_delivery_failure": Value::Null,
        "verification_expected": false,
        "verification_ran": false,
        "verification_failed": false,
        "verification_plan": [],
        "verification_results": [],
        "verification_total": 0,
        "verification_completed": 0,
        "verification_passed_count": 0,
        "verification_failed_count": 0,
        "latest_verification_command": Value::Null,
        "latest_verification_failure": Value::Null,
        "capability_resolution": capability_resolution.clone(),
    });
    automation_reset_attempt_tool_failure_labels(&mut tool_telemetry);
    tool_telemetry
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
    automation_infer_selected_mcp_servers_with_source(
        allowed_servers,
        allowlist,
        enabled_server_names,
        requires_email_delivery,
    )
    .0
}

pub(crate) fn automation_infer_selected_mcp_servers_with_source(
    allowed_servers: &[String],
    allowlist: &[String],
    enabled_server_names: &[String],
    requires_email_delivery: bool,
) -> (Vec<String>, &'static str) {
    let mut selected_servers = automation_normalize_server_list(allowed_servers);
    selected_servers.extend(automation_selected_mcp_servers_from_allowlist(
        allowlist,
        enabled_server_names,
    ));
    selected_servers.sort();
    selected_servers.dedup();
    if !selected_servers.is_empty() {
        return (selected_servers, "policy");
    }
    if requires_email_delivery {
        return (enabled_server_names.to_vec(), "email_fallback");
    }
    (Vec::new(), "none")
}

fn automation_add_mcp_list_when_scoped(
    mut requested_tools: Vec<String>,
    has_selected_mcp_servers: bool,
) -> Vec<String> {
    if has_selected_mcp_servers && !requested_tools.iter().any(|tool| tool == "mcp_list") {
        requested_tools.push("mcp_list".to_string());
    }
    requested_tools
}

fn automation_connector_hint_text(node: &AutomationFlowNode) -> String {
    [
        node.objective.as_str(),
        node.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(Value::as_object)
            .and_then(|builder| builder.get("prompt"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ]
    .join("\n")
}

fn automation_tool_telemetry_selected_mcp_servers(tool_telemetry: &Value) -> Vec<String> {
    tool_telemetry
        .get("capability_resolution")
        .and_then(|value| value.get("mcp_tool_diagnostics"))
        .and_then(|value| value.get("selected_servers"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn automation_tool_telemetry_has_mcp_usage(tool_telemetry: &Value) -> bool {
    tool_telemetry
        .get("executed_tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools.iter().any(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .is_some_and(|tool| tool.starts_with("mcp."))
            })
        })
}

fn automation_node_is_mcp_grounded_citations_artifact(
    node: &AutomationFlowNode,
    tool_telemetry: &Value,
) -> bool {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if contract_kind != "citations" {
        return false;
    }
    let selected_servers = automation_tool_telemetry_selected_mcp_servers(tool_telemetry);
    if selected_servers.is_empty() && !enforcement::automation_node_prefers_mcp_servers(node) {
        return false;
    }
    automation_tool_telemetry_has_mcp_usage(tool_telemetry)
}

fn automation_text_mentions_mcp_server(text: &str, server_name: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let lowered_server = server_name.to_ascii_lowercase();
    let namespace = crate::http::mcp::mcp_namespace_segment(server_name);
    [
        lowered_server.clone(),
        lowered_server.replace('-', "_"),
        lowered_server.replace('-', " "),
        namespace.clone(),
        namespace.replace('_', "-"),
        format!("mcp.{namespace}"),
    ]
    .iter()
    .filter(|needle| !needle.trim().is_empty())
    .any(|needle| lowered.contains(needle))
}

fn automation_requested_server_scoped_mcp_tools(
    node: &AutomationFlowNode,
    selected_server_names: &[String],
) -> Vec<String> {
    let connector_hint_text = automation_connector_hint_text(node);
    if !tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
        &connector_hint_text,
    ) {
        return Vec::new();
    }
    let mut requested = selected_server_names
        .iter()
        .filter(|server_name| {
            automation_text_mentions_mcp_server(&connector_hint_text, server_name)
        })
        .map(|server_name| {
            format!(
                "mcp.{}.*",
                crate::http::mcp::mcp_namespace_segment(server_name)
            )
        })
        .collect::<Vec<_>>();
    requested.sort();
    requested.dedup();
    requested
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
    let (selected_servers, selected_source) = automation_infer_selected_mcp_servers_with_source(
        allowed_servers,
        allowlist,
        &enabled_server_names,
        automation_node_requires_email_delivery(node),
    );
    if selected_servers.is_empty() {
        return json!({
            "selected_servers": [],
            "selected_source": "none",
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
        "selected_source": selected_source,
        "servers": server_rows,
        "remote_tools": all_remote_names,
        "registered_tools": all_registered_names,
        "remote_email_like_tools": all_remote_email_like_names,
        "registered_email_like_tools": all_registered_email_like_names,
    })
}

fn automation_node_delivery_method_value(node: &AutomationFlowNode) -> String {
    node_runtime_impl::automation_node_delivery_method(node).unwrap_or_else(|| "none".to_string())
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
    run_id: &str,
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
        .map(|path| automation_run_scoped_output_path(run_id, &path).unwrap_or(path))
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
        "- If you link to an artifact, use a canonical run-scoped path; if a safe href cannot be produced, render the path as plain text instead of a bare relative link.".to_string(),
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
    if automation_node_preserves_full_upstream_inputs(node) {
        lines.push("- The final deliverable body itself must remain substantive and complete; the concise requirement applies only to the wrapper response, not the report or email body.".to_string());
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

fn automation_prompt_canonicalize_artifact_hrefs(text: &str, run_id: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let canonical_root = automation_run_scoped_output_path(run_id, ".tandem/artifacts/")
        .unwrap_or_else(|| ".tandem/runs/unknown/artifacts".to_string())
        .trim_end_matches('/')
        .to_string();
    let canonical_prefix = format!("{canonical_root}/");
    trimmed
        .replace(
            "href=\".tandem/artifacts/",
            &format!("href=\"{canonical_prefix}"),
        )
        .replace(
            "href='.tandem/artifacts/",
            &format!("href='{canonical_prefix}"),
        )
        .replace(".tandem/artifacts/", &canonical_prefix)
}

fn automation_prompt_render_canonical_html_body(text: &str, run_id: &str) -> String {
    let trimmed = automation_prompt_canonicalize_artifact_hrefs(text, run_id);
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

fn render_deterministic_delivery_body(upstream_inputs: &[Value], run_id: &str) -> Option<String> {
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
    let source_path =
        automation_run_scoped_output_path(run_id, &source_path).unwrap_or(source_path);
    let rendered_html = automation_prompt_render_canonical_html_body(&text, run_id);
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
        -node_runtime_impl::automation_node_builder_priority(node),
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

pub(crate) fn automation_filter_runnable_by_routine_dependencies(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    runnable: Vec<AutomationFlowNode>,
) -> Vec<AutomationFlowNode> {
    runnable
        .into_iter()
        .filter(|node| {
            !node_runtime_impl::automation_node_routine_dependencies_blocked(automation, run, node)
        })
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
            if node_runtime_impl::automation_node_routine_dependencies_blocked(
                automation, run, node,
            ) {
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
    prompting_impl::render_automation_v2_prompt(
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
    )
}

pub(crate) fn render_automation_v2_prompt_with_options(
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
    prompting_impl::render_automation_v2_prompt_with_options(
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
        options,
    )
}

pub(crate) fn render_automation_repair_brief(
    node: &AutomationFlowNode,
    prior_output: Option<&Value>,
    attempt: u32,
    max_attempts: u32,
    run_id: Option<&str>,
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
    let tool_telemetry = prior_output
        .get("tool_telemetry")
        .cloned()
        .map(|mut value| {
            automation_reset_attempt_tool_failure_labels(&mut value);
            value
        });
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
    let mut blocking_classification = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unspecified")
        .to_string();
    let mut required_next_tool_actions = artifact_validation
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
    let validation_basis = artifact_validation
        .and_then(|value| value.get("validation_basis"))
        .and_then(Value::as_object);
    let current_attempt_has_recorded_activity = validation_basis
        .and_then(|basis| basis.get("current_attempt_has_recorded_activity"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let validation_basis_line = validation_basis
        .map(|basis| {
            let authority = basis
                .get("authority")
                .and_then(Value::as_str)
                .unwrap_or("unspecified");
            let current_attempt_output_materialized = basis
                .get("current_attempt_output_materialized")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_recorded_activity = basis
                .get("current_attempt_has_recorded_activity")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_read = basis
                .get("current_attempt_has_read")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_web_research = basis
                .get("current_attempt_has_web_research")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let workspace_inspection_satisfied = basis
                .get("workspace_inspection_satisfied")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            format!(
                "authority={}, output_materialized={}, recorded_activity={}, read={}, web_research={}, workspace_inspection={}",
                authority,
                current_attempt_output_materialized,
                current_attempt_has_recorded_activity,
                current_attempt_has_read,
                current_attempt_has_web_research,
                workspace_inspection_satisfied
            )
        })
        .unwrap_or_else(|| "none recorded".to_string());
    if blocking_classification == "execution_error" && current_attempt_has_recorded_activity {
        blocking_classification = "artifact_write_missing".to_string();
    }
    if current_attempt_has_recorded_activity
        && required_next_tool_actions.iter().any(|action| {
            action
                .to_ascii_lowercase()
                .contains("retry after provider connectivity recovers")
        })
    {
        required_next_tool_actions =
            vec!["write the required run artifact to the declared output path".to_string()];
    }
    let tools_offered = tool_telemetry
        .as_ref()
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
        .as_ref()
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
        if current_attempt_has_recorded_activity {
            "not recorded (but session activity was detected)".to_string()
        } else {
            "none recorded".to_string()
        }
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
    let final_attempt_line = if repair_attempts_remaining <= 1 {
        let output_path = automation_node_required_output_path_for_run(node, run_id)
            .unwrap_or_else(|| "the declared output path".to_string());
        format!(
            "\n\nFINAL ATTEMPT:\n- This is the last retry.\n- The engine will accept the output file at `{}` as-is if it exists when this attempt ends.\n- Do not ask follow-up questions.\n- Do not end with a summary.\n- Write the complete artifact to the output path and include {{\"status\":\"completed\"}} as the last line of your response.",
            output_path
        )
    } else {
        String::new()
    };

    Some(format!(
        "Repair Brief:\n- Node `{}` is being retried because the previous attempt ended in `needs_repair`.\n- Previous validation reason: {}.\n- Validation basis: {}.\n- Unmet requirements: {}.\n- Blocking classification: {}.\n- Required next tool actions: {}.\n- Tools offered last attempt: {}.\n- Tools executed last attempt: {}.\n- Relevant files still unread or explicitly unreviewed: {}.\n- Previous repair attempt count: {}.\n- Remaining repair attempts after this run: {}{}.\n- For this retry, satisfy the unmet requirements before finalizing the artifact.\n- Do not write a blocked handoff unless the required tools were actually attempted and remained unavailable or failed.{}",
        node.node_id,
        reason,
        validation_basis_line,
        unmet_line,
        blocking_classification,
        next_actions_line,
        tools_offered_line,
        tools_executed_line,
        unreviewed_line,
        repair_attempt,
        repair_attempts_remaining.saturating_sub(1),
        code_workflow_line,
        final_attempt_line,
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

/// Derives the receipt path from the standup report path by inserting a
/// "receipt-" prefix on the filename and replacing the extension with ".json".
/// Example: "docs/standups/2026-04-05.md" → "docs/standups/receipt-2026-04-05.json"
pub(super) fn standup_receipt_path_for_report(report_path: &str) -> String {
    let p = std::path::Path::new(report_path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("standup");
    let dir = p
        .parent()
        .and_then(|d| d.to_str())
        .filter(|d| !d.is_empty())
        .unwrap_or("docs/standups");
    format!("{dir}/receipt-{stem}.json")
}

/// Builds an operator-facing JSON receipt for a completed standup run.
/// Sources all data from existing structures: run checkpoint, lifecycle history,
/// node outputs, and the coordinator's assessment score.
/// Returns None if the run data is not available or this is not a standup run.
fn build_standup_run_receipt(
    run: &AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    run_id: &str,
    report_path: &str,
    coordinator_assessment: &ArtifactCandidateAssessment,
) -> Option<Value> {
    let completed_at_iso = run
        .finished_at_ms
        .or(run.started_at_ms)
        .map(|ms| {
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339()
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Count lifecycle events by type for summary
    let lifecycle_events = &run.checkpoint.lifecycle_history;
    let total_events = lifecycle_events.len();
    let total_repair_cycles = lifecycle_events
        .iter()
        .filter(|e| e.event == "node_repair_requested")
        .count();
    // Filler rejections are repair cycles on standup_update nodes
    let total_filler_rejections = lifecycle_events
        .iter()
        .filter(|e| {
            e.event == "node_repair_requested"
                && e.metadata
                    .as_ref()
                    .and_then(|m| m.get("contract_kind"))
                    .and_then(Value::as_str)
                    .is_some_and(|k| k == "standup_update")
        })
        .count();

    // Build per-participant summaries from node outputs
    let participants: Vec<Value> = automation
        .flow
        .nodes
        .iter()
        .filter(|n| n.node_id != "standup_synthesis")
        .map(|participant_node| {
            let node_output = run
                .checkpoint
                .node_outputs
                .get(&participant_node.node_id);
            let attempts = run
                .checkpoint
                .node_attempts
                .get(&participant_node.node_id)
                .copied()
                .unwrap_or(0);
            let status = node_output
                .and_then(|o| o.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            // Extract yesterday/today from the participant's standup JSON,
            // stored in the node output content text
            let standup_json = node_output
                .and_then(|o| o.get("content"))
                .and_then(|c| c.get("text").or_else(|| c.get("raw_assistant_text")))
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok());
            let yesterday = standup_json
                .as_ref()
                .and_then(|v| v.get("yesterday"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let today = standup_json
                .as_ref()
                .and_then(|v| v.get("today"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let filler_rejected = lifecycle_events.iter().any(|e| {
                e.event == "node_repair_requested"
                    && e.metadata
                        .as_ref()
                        .and_then(|m| m.get("node_id"))
                        .and_then(Value::as_str)
                        .is_some_and(|id| id == participant_node.node_id)
            });
            // Derive a readable name from the node_id (e.g., "participant_0_copywriter")
            let display_name = participant_node
                .node_id
                .splitn(3, '_')
                .nth(2)
                .unwrap_or(&participant_node.node_id)
                .replace('_', " ");
            json!({
                "node_id": participant_node.node_id,
                "display_name": display_name,
                "attempts": attempts,
                "status": status,
                "filler_rejected": filler_rejected,
                "yesterday_summary": if yesterday.is_empty() { Value::Null } else { json!(yesterday) },
                "today_summary": if today.is_empty() { Value::Null } else { json!(today) },
            })
        })
        .collect();

    let coordinator_attempts = run
        .checkpoint
        .node_attempts
        .get("standup_synthesis")
        .copied()
        .unwrap_or(0);

    Some(json!({
        "run_id": run_id,
        "automation_id": automation.automation_id,
        "automation_name": automation.name,
        "completed_at_iso": completed_at_iso,
        "report_path": report_path,
        "participants": participants,
        "coordinator": {
            "node_id": "standup_synthesis",
            "attempts": coordinator_attempts,
            "report_path": report_path,
            "assessment": assessment::artifact_candidate_summary(coordinator_assessment, true),
        },
        "lifecycle_event_count": total_events,
        "total_repair_cycles": total_repair_cycles,
        "total_filler_rejections": total_filler_rejections,
    }))
}

fn automation_workspace_project_id(workspace_root: &str) -> String {
    node_runtime_impl::automation_workspace_project_id(workspace_root)
}

fn merge_automation_agent_allowlist(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Vec<String> {
    node_runtime_impl::merge_automation_agent_allowlist(agent, template)
}

pub(crate) fn automation_node_output_contract_kind(node: &AutomationFlowNode) -> Option<String> {
    node.output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn automation_node_task_kind(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_kind(node)
}

fn automation_node_knowledge_task_family(node: &AutomationFlowNode) -> String {
    let explicit_family = automation_node_builder_metadata(node, "task_family")
        .or_else(|| automation_node_builder_metadata(node, "knowledge_task_family"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(family) = explicit_family {
        let normalized = tandem_orchestrator::normalize_knowledge_segment(&family);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    if let Some(task_kind) = automation_node_task_kind(node) {
        let mapped = match task_kind.as_str() {
            "code_change" | "repo_fix" | "implementation" | "debugging" | "bug_fix" => Some("code"),
            "research" | "analysis" | "synthesis" | "research_brief" => Some("research"),
            "support" | "ops" | "runbook" | "incident" | "triage" => Some("ops"),
            "plan" | "planning" | "roadmap" => Some("planning"),
            "verification" | "test" | "qa" => Some("verification"),
            _ => None,
        };
        if let Some(mapped) = mapped {
            return mapped.to_string();
        }
        let normalized = tandem_orchestrator::normalize_knowledge_segment(&task_kind);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    let workflow_class = automation_node_workflow_class(node);
    if workflow_class != "artifact" {
        return workflow_class;
    }

    if let Some(contract_kind) = automation_node_output_contract_kind(node) {
        let normalized = tandem_orchestrator::normalize_knowledge_segment(&contract_kind);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    let fallback = tandem_orchestrator::normalize_knowledge_segment(&node.node_id);
    if fallback.is_empty() {
        workflow_class
    } else {
        fallback
    }
}

fn automation_node_projects_backlog_tasks(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_projects_backlog_tasks(node)
}

fn automation_node_task_id(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_id(node)
}

fn automation_node_repo_root(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_repo_root(node)
}

fn automation_node_write_scope(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_write_scope(node)
}

fn automation_node_acceptance_criteria(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_acceptance_criteria(node)
}

fn automation_node_task_dependencies(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_dependencies(node)
}

fn automation_node_task_owner(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_owner(node)
}

fn automation_node_is_code_workflow(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_is_code_workflow(node)
}

pub(crate) fn automation_output_validator_kind(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputValidatorKind {
    node_runtime_impl::automation_output_validator_kind(node)
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
    node_runtime_impl::automation_node_execution_mode(node, workspace_root)
}

pub(crate) fn normalize_automation_requested_tools(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
) -> Vec<String> {
    let handoff_only_structured_json = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
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
    if handoff_only_structured_json {
        normalized.retain(|tool| !matches!(tool.as_str(), "write" | "edit" | "apply_patch"));
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

fn automation_tool_name_is_email_delivery(tool_name: &str) -> bool {
    node_runtime_impl::automation_tool_name_is_email_delivery(tool_name)
}

fn discover_automation_tools_for_capability(
    capability_id: &str,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    if available_tool_names.is_empty() {
        return vec!["*".to_string()];
    }
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
    automation_node_prewrite_requirements_impl(node, requested_tools)
}

fn automation_node_prewrite_requirements_impl(
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
    let optional_workspace_reads =
        enforcement::automation_node_allows_optional_workspace_reads(node);
    let explicit_input_files = automation_node_explicit_input_files(node);
    let has_required_read = required_tools.iter().any(|tool| tool == "read");
    let has_required_websearch = required_tools.iter().any(|tool| tool == "websearch");
    let has_any_required_tools = !required_tools.is_empty();
    let concrete_read_required = if !explicit_input_files.is_empty() {
        !research_finalize && requested_tools.iter().any(|tool| tool == "read")
    } else {
        !research_finalize
            && !optional_workspace_reads
            && ((brief_research_node || validation_profile == "local_research")
                || has_required_read
                || enforcement
                    .prewrite_gates
                    .iter()
                    .any(|gate| gate == "concrete_reads"))
            && requested_tools.iter().any(|tool| tool == "read")
    };
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
        workspace_inspection_required: workspace_inspection_required
            && !research_finalize
            && explicit_input_files.is_empty(),
        web_research_required: web_research_required && !research_finalize,
        concrete_read_required,
        successful_web_research_required,
        repair_on_unmet_requirements: brief_research_node
            || has_any_required_tools
            || !enforcement.retry_on_missing.is_empty(),
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
    } else if has_unmet("mcp_discovery_missing") {
        Some("connector-backed work completed without discovering available MCP tools".to_string())
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
    } else if has_unmet("bare_relative_artifact_href") {
        Some(
            "final artifact contains a bare relative artifact href; use a canonical run-scoped link or plain text instead"
                .to_string(),
        )
    } else if has_unmet("required_workspace_files_missing") {
        Some("required workspace files were not written for this run".to_string())
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

pub(crate) async fn resolve_automation_agent_model(
    state: &AppState,
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
    if let Some(model) = template
        .and_then(|value| value.default_model.as_ref())
        .and_then(crate::app::routines::parse_model_spec)
    {
        return Some(model);
    }

    let providers = state.providers.list().await;
    let effective_config = state.config.get_effective_value().await;
    if let Some(config_default) =
        crate::app::state::default_model_spec_from_effective_config(&effective_config)
            .filter(|spec| crate::app::routines::provider_catalog_has_model(&providers, spec))
    {
        return Some(config_default);
    }

    providers.into_iter().find_map(|provider| {
        let model = provider.models.first()?;
        Some(ModelSpec {
            provider_id: provider.id,
            model_id: model.id.clone(),
        })
    })
}

pub(crate) fn automation_node_inline_artifact_payload(node: &AutomationFlowNode) -> Option<Value> {
    node_runtime_impl::automation_node_inline_artifact_payload(node)
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

pub(crate) fn automation_node_required_output_path_for_run(
    node: &AutomationFlowNode,
    run_id: Option<&str>,
) -> Option<String> {
    node_runtime_impl::automation_node_required_output_path_for_run(node, run_id)
}

pub fn automation_node_required_output_path(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_required_output_path(node)
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

fn automation_node_explicit_input_files(node: &AutomationFlowNode) -> Vec<String> {
    let mut files = automation_node_builder_string_array(node, "input_files");
    files.sort();
    files.dedup();
    files
}

fn automation_node_explicit_output_files(node: &AutomationFlowNode) -> Vec<String> {
    let mut files = automation_node_builder_string_array(node, "output_files");
    files.sort();
    files.dedup();
    files
}

fn automation_node_must_write_files(node: &AutomationFlowNode) -> Vec<String> {
    let explicit_output_files = automation_node_explicit_output_files(node);
    if !explicit_output_files.is_empty() {
        return explicit_output_files;
    }
    let builder = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object);
    let explicit_must_write_files =
        builder.is_some_and(|builder| builder.contains_key("must_write_files"));
    let mut files = builder
        .and_then(|builder| builder.get("must_write_files"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !explicit_must_write_files {
        let inferred = automation_node_bootstrap_missing_files(node);
        if !inferred.is_empty() {
            tracing::warn!(
                node_id = %node.node_id,
                inferred_files = ?inferred,
                "automation bootstrap file inference is deprecated; set builder.must_write_files explicitly"
            );
            files.extend(inferred);
        }
    }
    files.sort();
    files.dedup();
    files
}

fn automation_runtime_placeholder_replace(
    text: &str,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> String {
    let Some(runtime_values) = runtime_values else {
        return text.to_string();
    };
    text.replace("{current_date}", &runtime_values.current_date)
        .replace("{current_time}", &runtime_values.current_time)
        .replace("{current_timestamp}", &runtime_values.current_timestamp)
}

fn automation_keyword_variants(token: &str) -> Vec<String> {
    let lowered = token.trim().to_ascii_lowercase();
    if lowered.len() < 3
        || lowered.chars().all(|ch| ch.is_ascii_digit())
        || matches!(
            lowered.as_str(),
            "md" | "json"
                | "jsonl"
                | "yaml"
                | "yml"
                | "txt"
                | "csv"
                | "toml"
                | "current"
                | "date"
                | "time"
                | "timestamp"
        )
    {
        return Vec::new();
    }
    let mut variants = vec![lowered.clone()];
    if let Some(stripped) = lowered.strip_suffix("ies") {
        if stripped.len() >= 2 {
            variants.push(format!("{stripped}y"));
        }
    } else if let Some(stripped) = lowered.strip_suffix('s') {
        if stripped.len() >= 3 {
            variants.push(stripped.to_string());
        }
    }
    variants.sort();
    variants.dedup();
    variants
}

fn automation_keyword_set(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .flat_map(automation_keyword_variants)
        .collect()
}

fn automation_output_target_matches_node_objective(
    output_target: &str,
    objective_text: &str,
) -> bool {
    let objective_lower = objective_text.to_ascii_lowercase();
    let output_lower = output_target.to_ascii_lowercase();
    if objective_lower.contains(&output_lower) {
        return true;
    }
    let basename = std::path::Path::new(output_target)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(output_target)
        .to_ascii_lowercase();
    if !basename.is_empty() && objective_lower.contains(&basename) {
        return true;
    }
    let objective_keywords = automation_keyword_set(objective_text);
    let target_keywords = automation_keyword_set(output_target);
    let overlap = target_keywords
        .intersection(&objective_keywords)
        .cloned()
        .collect::<HashSet<_>>();
    if overlap.len() >= 2 {
        return true;
    }
    overlap.iter().any(|keyword| {
        matches!(
            keyword.as_str(),
            "pipeline"
                | "shortlist"
                | "recap"
                | "ledger"
                | "finding"
                | "findings"
                | "overview"
                | "positioning"
                | "resume"
                | "target"
                | "state"
        )
    })
}

pub(crate) fn automation_node_must_write_files_for_automation(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> Vec<String> {
    let mut files = automation_node_must_write_files(node)
        .into_iter()
        .map(|path| automation_runtime_placeholder_replace(&path, runtime_values))
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn automation_node_bootstrap_missing_files(node: &AutomationFlowNode) -> Vec<String> {
    enforcement::automation_node_inferred_bootstrap_required_files(node)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutomationArtifactPublishScope {
    Workspace,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutomationArtifactPublishMode {
    SnapshotReplace,
    AppendJsonl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutomationArtifactPublishSpec {
    scope: AutomationArtifactPublishScope,
    path: String,
    mode: AutomationArtifactPublishMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationVerifiedOutputResolutionKind {
    Direct,
    LegacyPromoted,
    SessionTextRecovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomationVerifiedOutputResolution {
    path: PathBuf,
    legacy_workspace_artifact_promoted_from: Option<PathBuf>,
    materialized_by_current_attempt: bool,
    resolution_kind: AutomationVerifiedOutputResolutionKind,
}

fn automation_node_publish_spec(
    node: &AutomationFlowNode,
) -> Option<AutomationArtifactPublishSpec> {
    let publish = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("publish"))
        .and_then(Value::as_object)?;
    let scope = match publish
        .get("scope")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_ascii_lowercase()
        .as_str()
    {
        "workspace" => AutomationArtifactPublishScope::Workspace,
        "global" => AutomationArtifactPublishScope::Global,
        _ => return None,
    };
    let path = publish
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let mode = match publish
        .get("mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("snapshot_replace")
        .to_ascii_lowercase()
        .as_str()
    {
        "snapshot_replace" => AutomationArtifactPublishMode::SnapshotReplace,
        "append_jsonl" => AutomationArtifactPublishMode::AppendJsonl,
        _ => return None,
    };
    Some(AutomationArtifactPublishSpec { scope, path, mode })
}

fn automation_output_path_uses_legacy_workspace_artifact_contract(
    workspace_root: &str,
    output_path: &str,
) -> bool {
    let normalized = normalize_automation_path_text(output_path)
        .unwrap_or_else(|| output_path.trim().to_string())
        .replace('\\', "/");
    if normalized == ".tandem/artifacts" || normalized.starts_with(".tandem/artifacts/") {
        return true;
    }
    let Ok(resolved) = resolve_automation_output_path(workspace_root, output_path) else {
        return false;
    };
    let workspace = PathBuf::from(
        normalize_automation_path_text(workspace_root)
            .unwrap_or_else(|| workspace_root.trim().to_string()),
    );
    let Ok(relative) = resolved.strip_prefix(&workspace) else {
        return false;
    };
    let relative = normalize_automation_path_text(relative.to_string_lossy().as_ref())
        .unwrap_or_default()
        .replace('\\', "/");
    relative == ".tandem/artifacts" || relative.starts_with(".tandem/artifacts/")
}

fn maybe_promote_legacy_workspace_artifact_for_run(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<Option<AutomationVerifiedOutputResolution>> {
    if !automation_output_path_uses_legacy_workspace_artifact_contract(workspace_root, output_path)
    {
        return Ok(None);
    }
    if !session_write_touched_output_for_output(session, workspace_root, output_path, None) {
        return Ok(None);
    }

    let legacy_path = resolve_automation_output_path(workspace_root, output_path)?;
    let run_scoped_path =
        resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    if legacy_path == run_scoped_path {
        return Ok(None);
    }
    if !legacy_path.exists() || !legacy_path.is_file() {
        return Ok(None);
    }
    if let Some(parent) = run_scoped_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&legacy_path, &run_scoped_path).map_err(|error| {
        anyhow::anyhow!(
            "failed to promote legacy workspace artifact `{}` into run-scoped artifact `{}`: {}",
            legacy_path.display(),
            run_scoped_path.display(),
            error
        )
    })?;
    Ok(Some(AutomationVerifiedOutputResolution {
        path: run_scoped_path,
        legacy_workspace_artifact_promoted_from: Some(legacy_path),
        materialized_by_current_attempt: true,
        resolution_kind: AutomationVerifiedOutputResolutionKind::LegacyPromoted,
    }))
}

fn resolve_automation_published_output_path(
    workspace_root: &str,
    spec: &AutomationArtifactPublishSpec,
) -> anyhow::Result<PathBuf> {
    match spec.scope {
        AutomationArtifactPublishScope::Workspace => {
            resolve_automation_output_path(workspace_root, &spec.path)
        }
        AutomationArtifactPublishScope::Global => {
            let trimmed = spec.path.trim();
            if trimmed.is_empty() {
                anyhow::bail!("global publication path is empty");
            }
            let relative = PathBuf::from(trimmed);
            if relative.is_absolute() {
                anyhow::bail!(
                    "global publication path `{}` must be relative to the Tandem publication root",
                    trimmed
                );
            }
            let base = config::paths::resolve_automation_published_artifacts_dir();
            let candidate = base.join(relative);
            let normalized = PathBuf::from(
                normalize_automation_path_text(candidate.to_string_lossy().as_ref())
                    .unwrap_or_else(|| candidate.to_string_lossy().to_string()),
            );
            if !normalized.starts_with(&base) {
                anyhow::bail!(
                    "global publication path `{}` must stay inside `{}`",
                    trimmed,
                    base.display()
                );
            }
            Ok(normalized)
        }
    }
}

fn display_automation_published_output_path(
    workspace_root: &str,
    resolved: &PathBuf,
    spec: &AutomationArtifactPublishSpec,
) -> String {
    match spec.scope {
        AutomationArtifactPublishScope::Workspace => resolved
            .strip_prefix(workspace_root)
            .ok()
            .and_then(|value| value.to_str().map(str::to_string))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| spec.path.clone()),
        AutomationArtifactPublishScope::Global => resolved.to_string_lossy().to_string(),
    }
}

fn publish_automation_verified_output(
    workspace_root: &str,
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    verified_output: &(String, String),
    spec: &AutomationArtifactPublishSpec,
) -> anyhow::Result<Value> {
    let source_path = resolve_automation_output_path(workspace_root, &verified_output.0)?;
    let destination = resolve_automation_published_output_path(workspace_root, spec)?;
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if source_path == destination {
        return Ok(json!({
            "scope": match spec.scope {
                AutomationArtifactPublishScope::Workspace => "workspace",
                AutomationArtifactPublishScope::Global => "global",
            },
            "mode": match spec.mode {
                AutomationArtifactPublishMode::SnapshotReplace => "snapshot_replace",
                AutomationArtifactPublishMode::AppendJsonl => "append_jsonl",
            },
            "path": display_automation_published_output_path(workspace_root, &destination, spec),
            "source_artifact_path": verified_output.0,
            "appended_records": None::<u64>,
            "copied": false,
        }));
    }

    let mut appended_records = None;
    match spec.mode {
        AutomationArtifactPublishMode::SnapshotReplace => {
            std::fs::copy(&source_path, &destination).map_err(|error| {
                anyhow::anyhow!(
                    "failed to publish validated run artifact `{}` to `{}`: {}",
                    source_path.display(),
                    destination.display(),
                    error
                )
            })?;
        }
        AutomationArtifactPublishMode::AppendJsonl => {
            use std::io::Write;

            let content = std::fs::read_to_string(&source_path).map_err(|error| {
                anyhow::anyhow!(
                    "failed to read validated run artifact `{}` before publication: {}",
                    source_path.display(),
                    error
                )
            })?;
            let appended_record = json!({
                "automation_id": automation.automation_id,
                "run_id": run_id,
                "node_id": node.node_id,
                "source_artifact_path": verified_output.0,
                "published_at_ms": now_ms(),
                "content": serde_json::from_str::<Value>(&content).unwrap_or_else(|_| Value::String(content.clone())),
            });
            let line = serde_json::to_string(&appended_record)?;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&destination)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "failed to open publication target `{}` for append_jsonl: {}",
                        destination.display(),
                        error
                    )
                })?;
            writeln!(file, "{line}").map_err(|error| {
                anyhow::anyhow!(
                    "failed to append published run artifact to `{}`: {}",
                    destination.display(),
                    error
                )
            })?;
            appended_records = Some(1);
        }
    }

    Ok(json!({
        "scope": match spec.scope {
            AutomationArtifactPublishScope::Workspace => "workspace",
            AutomationArtifactPublishScope::Global => "global",
        },
        "mode": match spec.mode {
            AutomationArtifactPublishMode::SnapshotReplace => "snapshot_replace",
            AutomationArtifactPublishMode::AppendJsonl => "append_jsonl",
        },
        "path": display_automation_published_output_path(workspace_root, &destination, spec),
        "source_artifact_path": verified_output.0,
        "appended_records": appended_records,
        "copied": true,
    }))
}

fn automation_output_target_publish_specs(
    targets: &[String],
) -> Vec<AutomationArtifactPublishSpec> {
    let mut specs = Vec::new();
    let mut seen = HashSet::new();
    for target in targets {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = trimmed.strip_prefix("file://").unwrap_or(trimmed).trim();
        if normalized.is_empty() || normalized.contains("://") {
            continue;
        }
        let spec = AutomationArtifactPublishSpec {
            scope: AutomationArtifactPublishScope::Workspace,
            path: normalized.to_string(),
            mode: AutomationArtifactPublishMode::SnapshotReplace,
        };
        if seen.insert(spec.path.clone()) {
            specs.push(spec);
        }
    }
    specs
}

fn publish_automation_verified_outputs(
    workspace_root: &str,
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    verified_output: &(String, String),
) -> anyhow::Result<Value> {
    let publications = automation_output_target_publish_specs(&automation.output_targets)
        .into_iter()
        .map(|spec| {
            publish_automation_verified_output(
                workspace_root,
                automation,
                run_id,
                node,
                verified_output,
                &spec,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(json!({ "targets": publications }))
}

fn automation_node_web_research_expected(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_web_research_expected(node)
}

pub(crate) fn automation_node_required_tools(node: &AutomationFlowNode) -> Vec<String> {
    node_runtime_impl::automation_node_required_tools(node)
}

pub(crate) fn automation_node_execution_policy(
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> Value {
    node_runtime_impl::automation_node_execution_policy(node, workspace_root)
}

fn resolve_automation_output_path(
    workspace_root: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let trimmed = output_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("required output path is empty");
    }
    let workspace = PathBuf::from(
        normalize_automation_path_text(workspace_root)
            .unwrap_or_else(|| workspace_root.trim().to_string()),
    );
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    let normalized_resolved = PathBuf::from(
        normalize_automation_path_text(resolved.to_string_lossy().as_ref())
            .unwrap_or_else(|| resolved.to_string_lossy().to_string()),
    );
    if !normalized_resolved.starts_with(&workspace) {
        anyhow::bail!(
            "required output path `{}` must stay inside workspace `{}`",
            trimmed,
            workspace_root
        );
    }
    Ok(normalized_resolved)
}

fn normalize_automation_path_text(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    let is_absolute = path.is_absolute();
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !is_absolute {
                    normalized.push("..");
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    let normalized = normalized.to_string_lossy().trim().to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
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
    let normalized = normalize_automation_path_text(output_path)?.replace('\\', "/");
    let prefix = ".tandem/artifacts/";
    if let Some(suffix) = normalized.strip_prefix(prefix) {
        let root = automation_run_artifact_root(run_id)?;
        return Some(if suffix.is_empty() {
            root
        } else {
            format!("{root}/{suffix}")
        });
    }
    Some(normalized)
}

fn automation_run_scoped_absolute_output_path(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> Option<String> {
    let candidate = PathBuf::from(normalize_automation_path_text(output_path)?);
    if !candidate.is_absolute() {
        return None;
    }
    let workspace = PathBuf::from(normalize_automation_path_text(workspace_root)?);
    let relative = candidate.strip_prefix(&workspace).ok()?;
    let relative_text =
        normalize_automation_path_text(relative.to_string_lossy().as_ref())?.replace('\\', "/");
    if relative_text == ".tandem/artifacts" {
        return automation_run_artifact_root(run_id);
    }
    let suffix = relative_text.strip_prefix(".tandem/artifacts/")?;
    let root = automation_run_artifact_root(run_id)?;
    Some(if suffix.is_empty() {
        root
    } else {
        format!("{root}/{suffix}")
    })
}

pub(crate) fn resolve_automation_output_path_for_run(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let scoped_output_path =
        automation_run_scoped_absolute_output_path(workspace_root, run_id, output_path)
            .or_else(|| automation_run_scoped_output_path(run_id, output_path))
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

async fn reconcile_automation_resolve_verified_output_path(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
    max_wait_ms: u64,
    poll_interval_ms: u64,
) -> anyhow::Result<Option<AutomationVerifiedOutputResolution>> {
    let output_touched =
        session_write_touched_output_for_output(session, workspace_root, output_path, Some(run_id));
    let poll_interval_ms = poll_interval_ms.max(1);
    let start_ms = now_ms() as u64;

    loop {
        let candidates =
            automation_output_path_candidates(workspace_root, run_id, node, output_path)?;
        let session_written_candidates =
            session_write_paths_for_output_candidates(session, workspace_root, run_id, &candidates);
        if let Some(resolved) = automation_resolve_verified_output_path(
            session,
            workspace_root,
            run_id,
            node,
            output_path,
        )? {
            let materialized_by_current_attempt = session_written_candidates
                .iter()
                .any(|candidate| candidate == &resolved);
            return Ok(Some(AutomationVerifiedOutputResolution {
                path: resolved,
                legacy_workspace_artifact_promoted_from: None,
                materialized_by_current_attempt,
                resolution_kind: AutomationVerifiedOutputResolutionKind::Direct,
            }));
        }
        if let Some(promoted) = maybe_promote_legacy_workspace_artifact_for_run(
            session,
            workspace_root,
            run_id,
            output_path,
        )? {
            return Ok(Some(AutomationVerifiedOutputResolution {
                materialized_by_current_attempt: output_touched
                    || promoted.materialized_by_current_attempt,
                ..promoted
            }));
        }
        if let Some(recovered) =
            recover_required_output_from_session_text(session, workspace_root, run_id, output_path)?
        {
            return Ok(Some(AutomationVerifiedOutputResolution {
                path: recovered,
                legacy_workspace_artifact_promoted_from: None,
                materialized_by_current_attempt: true,
                resolution_kind: AutomationVerifiedOutputResolutionKind::SessionTextRecovery,
            }));
        }
        if !output_touched {
            return Ok(None);
        }
        let elapsed_ms = now_ms() as u64 - start_ms;
        if elapsed_ms >= max_wait_ms {
            return Ok(None);
        }
        let sleep_ms = poll_interval_ms.min(max_wait_ms.saturating_sub(elapsed_ms));
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }
}

fn recover_required_output_from_session_text(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let resolved = resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    let Some(extension) = resolved.extension().and_then(|value| value.to_str()) else {
        return Ok(None);
    };
    if !extension.eq_ignore_ascii_case("json") {
        return Ok(None);
    }
    let payload = extract_recoverable_json_from_session(session);
    let Some(payload) = payload else {
        return Ok(None);
    };
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&payload)?;
    std::fs::write(&resolved, serialized)?;
    Ok(Some(resolved))
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

fn should_downgrade_auto_cleaned_marker_rejection(
    rejected_reason: Option<&str>,
    auto_cleaned: bool,
    semantic_block_reason: Option<&str>,
    accepted_output_present: bool,
) -> bool {
    auto_cleaned
        && semantic_block_reason.is_none()
        && accepted_output_present
        && rejected_reason
            .is_some_and(|reason| reason.starts_with("undeclared marker files created:"))
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
    let target_path =
        automation_session_write_target_path(workspace_root, declared_output_path, run_id);
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
            let Some(path) = automation_write_arg_path(&args) else {
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
            let Some(content) = automation_write_arg_content(&args) else {
                continue;
            };
            if !content.trim().is_empty() {
                candidates.push(content.to_string());
            }
        }
    }
    candidates
}

fn automation_session_write_target_path(
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
) -> Option<PathBuf> {
    run_id
        .and_then(|run_id| {
            resolve_automation_output_path_for_run(workspace_root, run_id, declared_output_path)
                .ok()
        })
        .or_else(|| resolve_automation_output_path(workspace_root, declared_output_path).ok())
}

pub(crate) fn session_write_touched_output_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
) -> bool {
    let target_path =
        automation_session_write_target_path(workspace_root, declared_output_path, run_id);
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
            let Some(path) = automation_write_arg_path(&args) else {
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

pub(crate) fn session_write_materialized_output_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
) -> bool {
    let target_path =
        automation_session_write_target_path(workspace_root, declared_output_path, run_id);
    let Some(target_path) = target_path else {
        return false;
    };
    if !session_write_touched_output_for_output(
        session,
        workspace_root,
        declared_output_path,
        run_id,
    ) {
        return false;
    }
    std::fs::metadata(&target_path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn automation_verified_output_differs_from_preexisting(
    preexisting_output: Option<&str>,
    verified_output: &(String, String),
) -> bool {
    preexisting_output.is_none_or(|previous| previous != verified_output.1)
}

fn automation_repair_output_differs_from_preexisting(
    preexisting_output: Option<&str>,
    accepted_output: Option<&(String, String)>,
) -> bool {
    accepted_output.is_some_and(|output| {
        automation_verified_output_differs_from_preexisting(preexisting_output, output)
    })
}

fn automation_write_arg_path(args: &serde_json::Map<String, Value>) -> Option<&str> {
    args.get("path")
        .or_else(|| args.get("filePath"))
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("filepath"))
        .or_else(|| args.get("output_path"))
        .or_else(|| args.get("target_path"))
        .or_else(|| args.get("file"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn automation_write_arg_content(args: &serde_json::Map<String, Value>) -> Option<&str> {
    args.get("content")
        .or_else(|| args.get("contents"))
        .or_else(|| args.get("text"))
        .or_else(|| args.get("body"))
        .or_else(|| args.get("value"))
        .or_else(|| args.get("data"))
        .and_then(Value::as_str)
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
    let automation = AutomationV2Spec {
        automation_id: "validation".to_string(),
        name: "validation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Draft,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: None,
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "validation".to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    validate_automation_artifact_output_with_context(
        &automation,
        node,
        session,
        workspace_root,
        run_id,
        None,
        session_text,
        tool_telemetry,
        preexisting_output,
        verified_output,
        workspace_snapshot_before,
        upstream_evidence,
    )
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
    let verified_output_materialized = verified_output.as_ref().is_some_and(|value| {
        tool_telemetry
            .get("verified_output_materialized_by_current_attempt")
            .and_then(Value::as_bool)
            .unwrap_or(true)
            && automation_verified_output_differs_from_preexisting(preexisting_output, value)
    });
    let mut accepted_output = verified_output;
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
    let explicit_input_files = automation_node_explicit_input_files(node);
    let explicit_output_files = automation_node_explicit_output_files(node);
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
        let connector_discovery_text = automation_connector_hint_text(node);
        let connector_discovery_required =
            tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
                &connector_discovery_text,
            );
        let selected_mcp_server_names = tool_telemetry
            .get("capability_resolution")
            .and_then(|value| value.get("mcp_tool_diagnostics"))
            .and_then(|value| value.get("selected_servers"))
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
        let executed_concrete_mcp_tool = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| {
                tools.iter().filter_map(Value::as_str).any(|tool_name| {
                    tool_name != "mcp_list"
                        && connector_action_patterns.iter().any(|pattern| {
                            tandem_core::tool_name_matches_policy(pattern, tool_name)
                        })
                })
            });
        let workspace_inspection_satisfied = tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || executed_has_read
            || (use_upstream_evidence && !discovered_relevant_paths.is_empty());
        if connector_discovery_required
            && !executed_has_mcp_list
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
        let required_output_path = automation_node_required_output_path_for_run(node, run_id);
        let current_attempt_output_materialized_via_filesystem =
            required_output_path.as_ref().is_some_and(|output_path| {
                session_write_materialized_output_for_output(
                    session,
                    workspace_root,
                    output_path,
                    run_id,
                )
            });
        let current_attempt_has_recorded_activity = !executed_tools_for_attempt.is_empty()
            || !session_write_candidates.is_empty()
            || verified_output_materialized
            || (use_upstream_evidence && upstream_evidence.is_some());
        let preexisting_output_reuse_allowed =
            automation_node_allows_preexisting_output_reuse(node);
        let current_attempt_output_materialized =
            current_attempt_output_materialized_via_filesystem || verified_output_materialized;
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
                );
                let materialized_by_current_attempt = session_write_materialized_output_for_output(
                    session,
                    workspace_root,
                    required_path,
                    None,
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
                )),
            );
            object.insert(
                "current_attempt_has_recorded_activity".to_string(),
                json!(current_attempt_has_recorded_activity),
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
        let requires_local_source_reads =
            requires_local_source_reads && !mcp_grounded_citations_artifact;
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
            let missing_concrete_reads =
                !optional_workspace_reads && requires_local_source_reads && !executed_has_read;
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
            let preserve_current_attempt_output_missing = !current_attempt_output_materialized
                && unmet_requirements
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
            let missing_concrete_reads =
                !optional_workspace_reads && requires_read && !executed_has_read;
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
            let bare_relative_artifact_href =
                matches!(contract_kind.as_str(), "report_markdown" | "text_summary")
                    && selected.as_ref().is_some_and(|assessment| {
                        contains_bare_tandem_artifact_href(&assessment.text)
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
            && session_write_touched_output_for_output(session, workspace_root, &path, run_id);
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
            unmet_requirements.clear();
            repair_succeeded = true;
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
        let connector_discovery_required =
            tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
                &connector_discovery_text,
            );
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
        let optional_workspace_reads =
            enforcement::automation_node_allows_optional_workspace_reads(node);

        if structured_handoff.is_none() {
            unmet_requirements.push("structured_handoff_missing".to_string());
        }
        if requires_workspace_inspection && !workspace_inspection_satisfied {
            unmet_requirements.push("workspace_inspection_required".to_string());
        }
        if !optional_workspace_reads
            && (requires_read || requires_concrete_reads)
            && !executed_has_read
        {
            unmet_requirements.push("no_concrete_reads".to_string());
        }
        if !optional_workspace_reads && requires_concrete_reads && !executed_has_read {
            unmet_requirements.push("concrete_read_required".to_string());
        }
        if (requires_websearch || requires_successful_web_research) && !web_research_succeeded {
            unmet_requirements.push("missing_successful_web_research".to_string());
        }
        if connector_discovery_required
            && !executed_has_mcp_list
            && !enforcement::automation_node_prefers_mcp_servers(node)
        {
            unmet_requirements.push("mcp_discovery_missing".to_string());
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
    let required_output_path_for_retry = automation_node_required_output_path_for_run(node, run_id);
    let current_attempt_output_materialized_for_retry = required_output_path_for_retry
        .as_ref()
        .is_some_and(|output_path| {
            session_write_materialized_output_for_output(
                session,
                workspace_root,
                output_path,
                run_id,
            ) || verified_output_materialized
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
    }
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
        "validation_basis": validation_basis,
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

pub(crate) async fn run_automation_node_prompt_with_timeout<F>(
    state: &AppState,
    session_id: &str,
    node: &AutomationFlowNode,
    future: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>>,
{
    let timeout_ms = node
        .timeout_ms
        .filter(|value| *value > 0)
        .unwrap_or_else(|| match automation_output_validator_kind(node) {
            crate::AutomationOutputValidatorKind::StandupUpdate => 120_000,
            crate::AutomationOutputValidatorKind::StructuredJson => 180_000,
            _ => 600_000,
        });
    match tokio::time::timeout(Duration::from_millis(timeout_ms), future).await {
        Ok(result) => result,
        Err(_) => {
            let _ = state.cancellations.cancel(session_id).await;
            anyhow::bail!(
                "automation node `{}` timed out after {} ms",
                node.node_id,
                timeout_ms
            );
        }
    }
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
            resolve_automation_agent_template(state, &workspace_root, template_id)
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
    let selected_mcp_server_names = mcp_tool_diagnostics
        .get("selected_servers")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let selected_mcp_source = mcp_tool_diagnostics
        .get("selected_source")
        .and_then(Value::as_str)
        .unwrap_or("none")
        .to_string();
    let mut requested_tools = requested_tools;
    requested_tools.extend(automation_requested_server_scoped_mcp_tools(
        node,
        &selected_mcp_server_names,
    ));
    requested_tools.sort();
    requested_tools.dedup();
    let has_selected_mcp_servers_policy =
        !selected_mcp_server_names.is_empty() && selected_mcp_source == "policy";
    let requested_tools =
        automation_add_mcp_list_when_scoped(requested_tools, has_selected_mcp_servers_policy);
    let effective_offered_tools =
        automation_expand_effective_offered_tools(&requested_tools, &available_tool_names);
    let execution_mode = automation_node_execution_mode(node, &workspace_root);
    let mut capability_resolution = automation_resolve_capabilities_with_schemas(
        node,
        execution_mode,
        &effective_offered_tools,
        &available_tool_names,
        &available_tool_schemas,
    );
    if automation_node_requires_email_delivery(node) || has_selected_mcp_servers_policy {
        automation_merge_mcp_capability_diagnostics(
            &mut capability_resolution,
            &mcp_tool_diagnostics,
        );
    }
    let missing_capabilities =
        automation_capability_resolution_missing_capabilities(&capability_resolution);
    let offered_tool_schemas = available_tool_schemas
        .iter()
        .filter(|schema| {
            effective_offered_tools
                .iter()
                .any(|tool| tool == &schema.name)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing_capabilities.is_empty() {
        let offered_tools_summary = if effective_offered_tools.is_empty() {
            "none".to_string()
        } else {
            effective_offered_tools.join(", ")
        };
        let selected_servers_summary = {
            let servers = mcp_tool_diagnostics
                .get("selected_servers")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if servers.is_empty() {
                "none".to_string()
            } else {
                servers.join(", ")
            }
        };
        let registered_tools_summary = {
            let tools = mcp_tool_diagnostics
                .get("registered_tools")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if tools.is_empty() {
                "none".to_string()
            } else {
                tools.join(", ")
            }
        };
        let detail = format!(
            "required automation capabilities were not offered after MCP/tool sync: {}. Offered tools: {}. Selected MCP servers: {}. Registered MCP tools after sync: {}.",
            missing_capabilities.join(", "),
            offered_tools_summary,
            selected_servers_summary,
            registered_tools_summary
        );
        let mut output =
            crate::automation_v2::executor::build_node_execution_error_output_with_category(
                node,
                &detail,
                false,
                "tool_resolution_failed",
            );
        if let Some(object) = output.as_object_mut() {
            object.insert(
                "tool_telemetry".to_string(),
                automation_initialized_attempt_tool_telemetry(
                    &requested_tools,
                    &capability_resolution,
                ),
            );
            object.insert(
                "capability_resolution".to_string(),
                capability_resolution.clone(),
            );
        }
        return Ok(output);
    }
    state
        .set_automation_v2_session_mcp_servers(&session_id, selected_mcp_server_names.clone())
        .await;
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, requested_tools.clone())
        .await;
    state
        .engine_loop
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    let model = resolve_automation_agent_model(state, agent, template.as_ref()).await;
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
    // P1: Delta-aware standup — read the most recent previous standup report and inject it.
    // This gives both participants and the coordinator awareness of what was already reported,
    // allowing them to report only new progress rather than re-discovering the same workspace state.
    let previous_standup_context: Option<String> = if is_agent_standup_automation(automation) {
        let report_path_template = resolve_standup_report_path_template(automation);
        let run_ts = run.started_at_ms.unwrap_or_else(now_ms);
        let previous_report = report_path_template.and_then(|template| {
            // Try up to 7 days back to find the most recent previous report
            for days_back in 1u64..=7 {
                let previous_ts = run_ts.saturating_sub(days_back * 24 * 60 * 60 * 1000);
                let candidate_path = if template.contains("{{date}}") {
                    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
                        previous_ts as i64,
                    )
                    .unwrap_or_else(chrono::Utc::now)
                    .format("%Y-%m-%d")
                    .to_string();
                    template.replace("{{date}}", &date)
                } else {
                    break;
                };
                if let Ok(resolved) =
                    resolve_automation_output_path(&workspace_root, &candidate_path)
                {
                    if resolved.is_file() {
                        if let Ok(content) = std::fs::read_to_string(&resolved) {
                            let trimmed = content.trim();
                            if !trimmed.is_empty() {
                                return Some(format!(
                                    "Previous Standup Report ({}):\n{}\n\nReport only NEW progress since the above. Do not repeat items already listed in the previous standup.",
                                    candidate_path,
                                    trimmed
                                ));
                            }
                        }
                    }
                }
            }
            None
        });
        previous_report
    } else {
        None
    };
    let knowledge_preflight =
        automation_knowledge_preflight(state, automation, node, run_id, &project_id).await;
    let knowledge_context = {
        let base = knowledge_preflight.as_ref().and_then(|preflight| {
            if !preflight.is_reusable() {
                return None;
            }
            let rendered = preflight.format_for_injection();
            if rendered.trim().is_empty() {
                None
            } else {
                Some(rendered)
            }
        });
        match (base, previous_standup_context) {
            (Some(base), Some(prev)) => Some(format!("{base}\n\n{prev}")),
            (Some(base), None) => Some(base),
            (None, Some(prev)) => Some(prev),
            (None, None) => None,
        }
    };
    let max_attempts = automation_node_max_attempts(node);
    let runtime_values = automation_prompt_runtime_values(run.started_at_ms);
    let mut prompt = render_automation_v2_prompt_with_options(
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
            summary_only_upstream: false,
            knowledge_context: knowledge_context.clone(),
            runtime_values: Some(runtime_values.clone()),
        },
    );
    let preserve_full_upstream_inputs = automation_node_preserves_full_upstream_inputs(node);
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
        if preserve_full_upstream_inputs {
            preflight = build_automation_prompt_preflight(
                &prompt,
                &effective_offered_tools,
                &offered_tool_schemas,
                execution_mode,
                &capability_resolution,
                "full_upstream_preserved",
                true,
            );
        } else {
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
                    knowledge_context: knowledge_context.clone(),
                    runtime_values: Some(runtime_values.clone()),
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
    }
    if let Some(repair_brief) = render_automation_repair_brief(
        node,
        run.checkpoint.node_outputs.get(&node.node_id),
        attempt,
        max_attempts,
        Some(run_id),
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
    let result = run_automation_node_prompt_with_timeout(
        state,
        &session_id,
        node,
        state.engine_loop.run_prompt_async_with_context(
            session_id.clone(),
            req,
            Some(format!("automation-v2:{run_id}")),
        ),
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
    state
        .clear_automation_v2_session_mcp_servers(&session_id)
        .await;
    state.clear_automation_v2_session(run_id, &session_id).await;

    result?;
    let expect_tool_activity = !requested_tools.is_empty();
    let session = load_automation_session_after_run(state, &session_id, expect_tool_activity)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation session `{}` missing after run", session_id))?;
    let session_text = extract_session_text_output(&session);
    let verified_output = if let Some(output_path) = required_output_path.as_deref() {
        let resolution = reconcile_automation_resolve_verified_output_path(
            &session,
            &workspace_root,
            run_id,
            node,
            output_path,
            250,
            25,
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "required output `{}` was not created for node `{}`",
                output_path,
                node.node_id
            )
        })?;
        let resolved = resolution.path.clone();
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
        Some((display_path, file_text, resolution))
    } else {
        None
    };
    let tool_telemetry = summarize_automation_tool_activity(node, &session, &requested_tools);
    let mut tool_telemetry = tool_telemetry;
    let verified_output_resolution = verified_output
        .as_ref()
        .map(|(_, _, resolution)| resolution.clone());
    let verified_output_for_evidence = verified_output
        .as_ref()
        .map(|(path, text, _)| (path.clone(), text.clone()));
    let base_attempt_evidence = node_output::build_automation_attempt_evidence(
        node,
        attempt,
        &session,
        &session_id,
        &workspace_root,
        &tool_telemetry,
        &preflight,
        &capability_resolution,
        required_output_path.as_deref(),
        verified_output_resolution.as_ref(),
        verified_output_for_evidence.as_ref(),
    );
    if let Some(object) = tool_telemetry.as_object_mut() {
        object.insert("preflight".to_string(), preflight.clone());
        object.insert(
            "capability_resolution".to_string(),
            capability_resolution.clone(),
        );
        object.insert(
            "verified_output_materialized_by_current_attempt".to_string(),
            json!(verified_output_resolution
                .as_ref()
                .map(|resolution| resolution.materialized_by_current_attempt)
                .unwrap_or(false)),
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
    let verified_output = verified_output.map(|(path, text, _)| (path, text));
    let (verified_output, mut artifact_validation, artifact_rejected_reason) =
        validate_automation_artifact_output_with_context(
            automation,
            node,
            &session,
            &workspace_root,
            Some(run_id.as_ref()),
            Some(&runtime_values),
            &session_text,
            &tool_telemetry,
            preexisting_output.as_deref(),
            verified_output,
            &workspace_snapshot_before,
            upstream_evidence.as_ref(),
        );
    let _ = artifact_rejected_reason;
    if let Some(promoted_from) = verified_output_resolution
        .as_ref()
        .and_then(|resolution| resolution.legacy_workspace_artifact_promoted_from.as_ref())
    {
        if let Some(object) = artifact_validation.as_object_mut() {
            object.insert(
                "legacy_workspace_artifact_promoted".to_string(),
                json!(true),
            );
            object.insert(
                "legacy_workspace_artifact_promoted_from".to_string(),
                json!(promoted_from.to_string_lossy().to_string()),
            );
            object
                .entry("accepted_candidate_source".to_string())
                .or_insert_with(|| json!("legacy_workspace_artifact_promoted"));
        }
    }

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
    let artifact_publication = if artifact_validation
        .get("semantic_block_reason")
        .and_then(Value::as_str)
        .is_none()
    {
        if let Some(verified_output) = verified_output.as_ref() {
            if let Some(spec) = automation_node_publish_spec(node) {
                Some(
                    publish_automation_verified_output(
                        &workspace_root,
                        automation,
                        run_id,
                        node,
                        verified_output,
                        &spec,
                    )
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "durable publication failed for node `{}` after validating `{}`: {}",
                            node.node_id,
                            verified_output.0,
                            error
                        )
                    })?,
                )
            } else if !automation.output_targets.is_empty() {
                Some(
                    publish_automation_verified_outputs(
                        &workspace_root,
                        automation,
                        run_id,
                        node,
                        verified_output,
                    )
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "durable publication failed for node `{}` after validating `{}`: {}",
                            node.node_id,
                            verified_output.0,
                            error
                        )
                    })?,
                )
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    if let Some(publication) = artifact_publication.clone() {
        if let Some(object) = artifact_validation.as_object_mut() {
            object.insert("artifact_publication".to_string(), publication);
        }
    }
    let (receipt_status, receipt_blocked_reason, receipt_approved) =
        node_output::detect_automation_node_status(
            node,
            &session_text,
            verified_output.as_ref(),
            &tool_telemetry,
            Some(&artifact_validation),
        );
    let receipt_blocker_category = node_output::detect_automation_blocker_category(
        node,
        &receipt_status,
        receipt_blocked_reason.as_deref(),
        &tool_telemetry,
        Some(&artifact_validation),
    );
    let receipt_fallback_used =
        automation_attempt_uses_legacy_fallback(&session_text, Some(&artifact_validation));
    let receipt_validator_summary = node_output::build_automation_validator_summary(
        automation_output_validator_kind(node),
        &receipt_status,
        receipt_blocked_reason.as_deref(),
        Some(&artifact_validation),
    );
    let receipt_attempt_evidence = tool_telemetry
        .get("attempt_evidence")
        .cloned()
        .map(|value| {
            node_output::augment_automation_attempt_evidence_with_validation(
                &value,
                Some(&artifact_validation),
                verified_output.as_ref(),
                artifact_validation
                    .get("accepted_candidate_source")
                    .and_then(Value::as_str),
                receipt_blocker_category.as_deref(),
                receipt_fallback_used,
                node_output::automation_backend_actionability_state(&receipt_status),
            )
        });
    let receipt_telemetry_summary = json!({
        "receipt_kind": "tool_telemetry_summary",
        "automation_id": automation.automation_id,
        "automation_run_id": run_id,
        "context_run_id": format!("automation-v2-{run_id}"),
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "preflight": preflight.clone(),
        "capability_resolution": capability_resolution.clone(),
        "tool_call_counts": tool_telemetry.get("tool_call_counts").cloned().unwrap_or_else(|| json!({})),
        "web_research_used": tool_telemetry.get("web_research_used").cloned().unwrap_or_else(|| json!(false)),
        "web_research_succeeded": tool_telemetry.get("web_research_succeeded").cloned().unwrap_or_else(|| json!(false)),
        "latest_web_research_failure": tool_telemetry.get("latest_web_research_failure").cloned().unwrap_or(Value::Null),
        "email_delivery_attempted": tool_telemetry.get("email_delivery_attempted").cloned().unwrap_or_else(|| json!(false)),
        "email_delivery_succeeded": tool_telemetry.get("email_delivery_succeeded").cloned().unwrap_or_else(|| json!(false)),
        "latest_email_delivery_failure": tool_telemetry.get("latest_email_delivery_failure").cloned().unwrap_or(Value::Null),
    });
    let receipt_attempt_summary = json!({
        "receipt_kind": "attempt_summary",
        "automation_id": automation.automation_id,
        "automation_run_id": run_id,
        "context_run_id": format!("automation-v2-{run_id}"),
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "status": receipt_status,
        "approved": receipt_approved,
        "blocked_reason": receipt_blocked_reason,
        "blocker_category": receipt_blocker_category,
        "fallback_used": receipt_fallback_used,
        "attempt_evidence": receipt_attempt_evidence,
    });
    let receipt_validation_summary = json!({
        "receipt_kind": "validation_summary",
        "automation_id": automation.automation_id,
        "automation_run_id": run_id,
        "context_run_id": format!("automation-v2-{run_id}"),
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "validator_summary": receipt_validator_summary,
    });
    let mut receipt_events = collect_automation_attempt_receipt_events(
        automation,
        run_id,
        node,
        attempt,
        &session_id,
        &session,
        verified_output.as_ref(),
        verified_output_resolution.as_ref(),
        required_output_path.as_deref(),
        Some(&artifact_validation),
    );
    receipt_events.extend(vec![
        AutomationAttemptReceiptEventInput {
            event_type: "attempt_summary".to_string(),
            payload: receipt_attempt_summary,
        },
        AutomationAttemptReceiptEventInput {
            event_type: "tool_telemetry_summary".to_string(),
            payload: receipt_telemetry_summary,
        },
        AutomationAttemptReceiptEventInput {
            event_type: "validation_summary".to_string(),
            payload: receipt_validation_summary,
        },
    ]);
    let receipt_root = receipts::automation_attempt_receipts_root();
    let receipt_ledger = match append_automation_attempt_receipts(
        &receipt_root,
        run_id,
        &node.node_id,
        attempt,
        &session_id,
        &receipt_events,
    )
    .await
    {
        Ok(summary) => Some(serde_json::to_value(summary)?),
        Err(error) => {
            tracing::warn!(
                run_id = %run_id,
                node_id = %node.node_id,
                attempt = attempt,
                error = %error,
                "failed to append automation attempt receipt ledger"
            );
            None
        }
    };
    let receipt_timeline = receipt_ledger
        .as_ref()
        .and_then(|ledger| ledger.get("path").and_then(Value::as_str))
        .map(PathBuf::from);
    let receipt_timeline = match receipt_timeline {
        Some(path) => receipts::read_automation_attempt_receipt_records(&path)
            .await
            .ok()
            .map(|records| {
                json!({
                    "record_count": records.len(),
                    "records": records,
                })
            }),
        None => None,
    };
    let attempt_forensic_record = json!({
        "version": 1,
        "automation_id": automation.automation_id,
        "automation_run_id": run_id,
        "context_run_id": format!("automation-v2-{run_id}"),
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "status": receipt_status,
        "final_backend_actionability_state": node_output::automation_backend_actionability_state(&receipt_status),
        "approved": receipt_approved,
        "blocked_reason": receipt_blocked_reason,
        "blocker_category": receipt_blocker_category,
        "fallback_used": receipt_fallback_used,
        "preflight": preflight.clone(),
        "capability_resolution": capability_resolution.clone(),
        "validator_summary": receipt_validator_summary,
        "attempt_evidence": receipt_attempt_evidence.clone(),
        "receipt_ledger": receipt_ledger.clone(),
        "receipt_timeline": receipt_timeline.clone(),
    });
    let attempt_forensic_record_path = match receipts::persist_automation_attempt_forensic_record(
        &workspace_root,
        run_id,
        &node.node_id,
        attempt,
        &attempt_forensic_record,
    )
    .await
    {
        Ok(path) => Some(path.to_string_lossy().to_string()),
        Err(error) => {
            tracing::warn!(
                run_id = %run_id,
                node_id = %node.node_id,
                attempt = attempt,
                error = %error,
                "failed to persist automation attempt forensic record"
            );
            None
        }
    };
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
        if let Some(knowledge_preflight) = knowledge_preflight.as_ref() {
            object.insert(
                "knowledge_preflight".to_string(),
                serde_json::to_value(knowledge_preflight)?,
            );
        }
        if let Some(publication) = artifact_publication {
            object.insert("artifact_publication".to_string(), publication);
        }
        if let Some(receipt_timeline) = receipt_timeline.clone() {
            object.insert("receipt_timeline".to_string(), receipt_timeline);
        }
        if let Some(receipt_ledger) = receipt_ledger {
            if let Some(attempt_evidence) = object
                .get_mut("attempt_evidence")
                .and_then(Value::as_object_mut)
            {
                attempt_evidence.insert("receipt_ledger".to_string(), receipt_ledger);
                if let Some(receipt_timeline) = receipt_timeline {
                    attempt_evidence.insert("receipt_timeline".to_string(), receipt_timeline);
                }
            }
        }
        if let Some(path) = attempt_forensic_record_path.clone() {
            object.insert(
                "attempt_forensic_record_path".to_string(),
                json!(path.clone()),
            );
            if let Some(attempt_evidence) = object
                .get_mut("attempt_evidence")
                .and_then(Value::as_object_mut)
            {
                attempt_evidence.insert("forensic_record_path".to_string(), json!(path));
            }
        }
        if !external_actions.is_empty() {
            object.insert(
                "external_actions".to_string(),
                serde_json::to_value(&external_actions)?,
            );
        }

        // --- A. Standup coordinator assessment scoring ---
        // Reuses assess_artifact_candidate() from assessment.rs to score the
        // coordinator's synthesis report. Records score + breakdown as metadata.
        // Does NOT hard-block. Soft warning only for low-quality outputs.
        //
        // Score thresholds (informational, not enforcement gates):
        //   < 0   : effectively empty/broken
        //   < 500 : weak — warning logged + standup_quality_warning flag set
        //   >= 500: acceptable
        //   >= 2000: strong (substantive flag set by assess_artifact_candidate)
        if is_agent_standup_automation(automation) && node.node_id == "standup_synthesis" {
            let report_text = object
                .get("content")
                .and_then(|c| c.get("text").and_then(Value::as_str))
                .or_else(|| {
                    object
                        .get("content")
                        .and_then(|c| c.get("raw_assistant_text").and_then(Value::as_str))
                })
                .unwrap_or(&session_text);
            let read_paths = tool_telemetry
                .get("read_paths")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let assessment = assess_artifact_candidate(
                node,
                &workspace_root,
                "session_write",
                report_text,
                &read_paths,
                &[],
                &[],
                &[],
            );
            let assessment_summary = assessment::artifact_candidate_summary(&assessment, true);
            object.insert("standup_assessment".to_string(), assessment_summary);
            if assessment.score < 500 {
                object.insert("standup_quality_warning".to_string(), json!(true));
                tracing::warn!(
                    run_id = %run_id,
                    node_id = %node.node_id,
                    score = assessment.score,
                    substantive = assessment.substantive,
                    placeholder_like = assessment.placeholder_like,
                    "standup coordinator output scored below warning threshold (500); \
                     report may be low-quality"
                );
            }

            // --- B. Operator-facing standup run receipt ---
            // Generates a JSON receipt beside the standup report using existing
            // node_outputs, node_attempts, lifecycle_history, and assessment data.
            // The receipt path is derived from the report path by inserting a
            // "receipt-" prefix on the filename, e.g.:
            //   docs/standups/2026-04-05.md -> docs/standups/receipt-2026-04-05.json
            if let Some(report_path) = standup_report_path.as_deref() {
                if let Some(receipt_json) = build_standup_run_receipt(
                    &run_after,
                    automation,
                    run_id,
                    report_path,
                    &assessment,
                ) {
                    let receipt_path = standup_receipt_path_for_report(report_path);
                    let abs_receipt = PathBuf::from(&workspace_root).join(&receipt_path);
                    if let Some(parent) = abs_receipt.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match serde_json::to_string_pretty(&receipt_json) {
                        Ok(content) => match std::fs::write(&abs_receipt, &content) {
                            Ok(()) => {
                                object.insert(
                                    "standup_receipt_path".to_string(),
                                    json!(receipt_path),
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    run_id = %run_id,
                                    receipt_path = %receipt_path,
                                    error = %err,
                                    "failed to write standup run receipt"
                                );
                            }
                        },
                        Err(err) => {
                            tracing::warn!(
                                run_id = %run_id,
                                error = %err,
                                "failed to serialize standup run receipt"
                            );
                        }
                    }
                }
            }
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
