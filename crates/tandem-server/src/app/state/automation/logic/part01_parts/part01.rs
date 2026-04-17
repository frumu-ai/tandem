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
    pub(crate) current_date_compact: String,
    pub(crate) current_time_hms: String,
    pub(crate) current_timestamp_filename: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AutomationPromptRenderOptions {
    pub(crate) summary_only_upstream: bool,
    pub(crate) knowledge_context: Option<String>,
    pub(crate) runtime_values: Option<AutomationPromptRuntimeValues>,
}

pub(crate) fn automation_prompt_runtime_values(
    started_at_ms: Option<u64>,
) -> AutomationPromptRuntimeValues {
    let started_at_ms = started_at_ms.unwrap_or_else(now_ms);
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(started_at_ms as i64)
        .unwrap_or_else(chrono::Utc::now);
    AutomationPromptRuntimeValues {
        current_date: timestamp.format("%Y-%m-%d").to_string(),
        current_time: timestamp.format("%H%M").to_string(),
        current_timestamp: timestamp.format("%Y-%m-%d %H:%M").to_string(),
        current_date_compact: timestamp.format("%Y%m%d").to_string(),
        current_time_hms: timestamp.format("%H%M%S").to_string(),
        current_timestamp_filename: timestamp.format("%Y-%m-%d_%H-%M-%S").to_string(),
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

