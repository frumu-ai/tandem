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

pub(crate) const AUTOMATION_PROMPT_WARNING_TOKENS: u64 = 2_400;
pub(crate) const AUTOMATION_PROMPT_HIGH_TOKENS: u64 = 3_200;
pub(crate) const AUTOMATION_TOOL_SCHEMA_WARNING_CHARS: u64 = 18_000;
pub(crate) const AUTOMATION_TOOL_SCHEMA_HIGH_CHARS: u64 = 26_000;

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
    pub(crate) mcp_contract_guidance: Option<String>,
}

fn automation_schema_type_label(schema: &Value) -> String {
    if let Some(value) = schema.get("const") {
        return format!("const {}", value);
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let preview = values
            .iter()
            .take(4)
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join(" | ");
        if !preview.is_empty() {
            return format!("enum {preview}");
        }
    }
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        return kind.to_string();
    }
    if let Some(types) = schema.get("type").and_then(Value::as_array) {
        let joined = types
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" | ");
        if !joined.is_empty() {
            return joined;
        }
    }
    if schema.get("anyOf").is_some() {
        return "anyOf".to_string();
    }
    if schema.get("oneOf").is_some() {
        return "oneOf".to_string();
    }
    "unknown".to_string()
}

fn automation_schema_example_value(schema: &Value) -> Value {
    if let Some(value) = schema.get("const") {
        return value.clone();
    }
    if let Some(value) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return value.clone();
    }
    let schema_type = schema.get("type");
    let type_name = schema_type
        .and_then(Value::as_str)
        .or_else(|| {
            schema_type.and_then(Value::as_array).and_then(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|kind| *kind != "null")
            })
        })
        .unwrap_or("string");
    match type_name {
        "array" => json!([]),
        "boolean" => json!(false),
        "integer" | "number" => json!(0),
        "null" => Value::Null,
        "object" => json!({}),
        "string" => {
            let min_length = schema.get("minLength").and_then(Value::as_u64).unwrap_or(0);
            if min_length > 0 {
                json!("search")
            } else {
                json!("")
            }
        }
        _ => json!(""),
    }
}

fn automation_mcp_contract_for_schema(schema: &ToolSchema) -> Option<Value> {
    if !schema.name.starts_with("mcp.") || schema.name == "mcp_list" {
        return None;
    }
    let input = &schema.input_schema;
    let mut warnings = Vec::<String>::new();
    let object = input.as_object();
    if object.is_none() {
        warnings.push("input_schema is not an object".to_string());
    }
    let properties = input
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = input
        .get("required")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !required.is_empty() && properties.is_empty() {
        warnings.push("required fields are declared but properties are missing".to_string());
    }
    if input.get("anyOf").is_some() || input.get("oneOf").is_some() {
        warnings.push(
            "root schema uses anyOf/oneOf; arguments may need tool-specific care".to_string(),
        );
    }

    let mut required_args = Vec::<Value>::new();
    let mut optional_args = Vec::<Value>::new();
    let mut example = serde_json::Map::new();
    for field in &required {
        let Some(prop) = properties.get(field) else {
            warnings.push(format!(
                "required field `{field}` is missing from properties"
            ));
            continue;
        };
        if !prop.is_object() {
            warnings.push(format!("required field `{field}` schema is not an object"));
        }
        if prop.get("type").is_none()
            && prop.get("enum").is_none()
            && prop.get("const").is_none()
            && prop.get("anyOf").is_none()
            && prop.get("oneOf").is_none()
        {
            warnings.push(format!("required field `{field}` has no clear type"));
        }
        if automation_schema_type_label(prop) == "string"
            && prop.get("minLength").and_then(Value::as_u64).unwrap_or(0) > 0
        {
            warnings.push(format!(
                "required string field `{field}` has minLength {}; do not pass an empty string",
                prop.get("minLength").and_then(Value::as_u64).unwrap_or(0)
            ));
        }
        required_args.push(json!({
            "name": field,
            "type": automation_schema_type_label(prop),
            "description": prop.get("description").and_then(Value::as_str).unwrap_or(""),
        }));
        example.insert(field.clone(), automation_schema_example_value(prop));
    }
    let required_set = required.iter().cloned().collect::<HashSet<_>>();
    for (field, prop) in &properties {
        if required_set.contains(field) {
            continue;
        }
        optional_args.push(json!({
            "name": field,
            "type": automation_schema_type_label(prop),
            "description": prop.get("description").and_then(Value::as_str).unwrap_or(""),
        }));
    }
    Some(json!({
        "tool": schema.name,
        "description": schema.description,
        "required_args": required_args,
        "optional_args": optional_args,
        "minimal_args_example": Value::Object(example),
        "schema_warnings": warnings,
    }))
}

pub(crate) fn automation_mcp_contract_summaries(offered_tool_schemas: &[ToolSchema]) -> Value {
    let contracts = offered_tool_schemas
        .iter()
        .filter_map(automation_mcp_contract_for_schema)
        .collect::<Vec<_>>();
    let warning_count = contracts
        .iter()
        .filter_map(|contract| contract.get("schema_warnings").and_then(Value::as_array))
        .map(Vec::len)
        .sum::<usize>();
    json!({
        "policy": "warn_then_run",
        "contract_count": contracts.len(),
        "warning_count": warning_count,
        "contracts": contracts,
    })
}

pub(crate) fn automation_required_tool_call_arg_validation(
    required_calls: &[AutomationRequiredToolCall],
    offered_tool_schemas: &[ToolSchema],
) -> Value {
    let mut rows = Vec::new();
    for call in required_calls {
        if !call.tool.starts_with("mcp.") || call.tool.ends_with(".*") {
            continue;
        }
        let Some(schema) = offered_tool_schemas
            .iter()
            .find(|schema| schema.name == call.tool)
        else {
            rows.push(json!({
                "tool": call.tool,
                "status": "tool_schema_missing",
                "warnings": ["required tool schema was not offered"],
            }));
            continue;
        };
        let contract = automation_mcp_contract_for_schema(schema).unwrap_or_else(|| json!({}));
        let args = call.args.clone().unwrap_or_else(|| json!({}));
        let mut warnings = Vec::<String>::new();
        let required_args = contract
            .get("required_args")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for arg in required_args {
            let Some(name) = arg.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !args
                .as_object()
                .is_some_and(|object| object.contains_key(name))
            {
                warnings.push(format!("static args missing required field `{name}`"));
            }
        }
        rows.push(json!({
            "tool": call.tool,
            "status": if warnings.is_empty() { "ok" } else { "warning" },
            "warnings": warnings,
            "minimal_args_example": contract.get("minimal_args_example").cloned().unwrap_or_else(|| json!({})),
        }));
    }
    json!(rows)
}

pub(crate) fn automation_mcp_contract_prompt_guidance(mcp_contracts: &Value) -> Option<String> {
    let contracts = mcp_contracts
        .get("contracts")
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())?;
    let mut lines = vec![
        "MCP Tool Contracts:".to_string(),
        "- Use these argument contracts before calling MCP tools. If a required field is listed, include it in the tool args; do not omit required string fields when an empty string is the intended value.".to_string(),
        "- Schema warnings are non-blocking, but treat them as a reason to be conservative and inspect tool errors carefully.".to_string(),
    ];
    for contract in contracts.iter().take(8) {
        let tool = contract.get("tool").and_then(Value::as_str).unwrap_or("");
        if tool.is_empty() {
            continue;
        }
        let required = contract
            .get("required_args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .filter_map(|arg| {
                        Some(format!(
                            "{}: {}",
                            arg.get("name").and_then(Value::as_str)?,
                            arg.get("type").and_then(Value::as_str).unwrap_or("unknown")
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "none".to_string());
        let example = contract
            .get("minimal_args_example")
            .map(Value::to_string)
            .unwrap_or_else(|| "{}".to_string());
        lines.push(format!(
            "- `{tool}` required args: {required}; minimal args example: `{example}`"
        ));
        if let Some(warnings) = contract.get("schema_warnings").and_then(Value::as_array) {
            let warning_text = warnings
                .iter()
                .filter_map(Value::as_str)
                .take(3)
                .collect::<Vec<_>>()
                .join("; ");
            if !warning_text.is_empty() {
                lines.push(format!("  warnings: {warning_text}"));
            }
        }
    }
    Some(lines.join("\n"))
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

pub(crate) fn automation_effective_knowledge_binding(
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
    let tenant_context = automation.tenant_context();
    let memory_tenant_scope = tandem_memory::types::MemoryTenantScope {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        deployment_id: tenant_context.deployment_id.clone(),
    };
    let preflight = manager
        .preflight_knowledge_for_tenant(
            &tandem_orchestrator::KnowledgePreflightRequest {
                project_id: project_id.to_string(),
                task_family: task_family.clone(),
                subject,
                binding,
            },
            &memory_tenant_scope,
        )
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

pub(crate) fn automation_attempt_evidence_from_tool_telemetry<'a>(
    tool_telemetry: &'a Value,
) -> Option<&'a Value> {
    tool_telemetry.get("attempt_evidence")
}

pub(crate) fn automation_attempt_evidence_read_paths(tool_telemetry: &Value) -> Vec<String> {
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

pub(crate) fn automation_attempt_evidence_web_research_status(
    tool_telemetry: &Value,
) -> Option<String> {
    automation_attempt_evidence_from_tool_telemetry(tool_telemetry)
        .and_then(|value| value.get("evidence"))
        .and_then(|value| value.get("web_research"))
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn automation_attempt_evidence_delivery_status(
    tool_telemetry: &Value,
) -> Option<String> {
    automation_attempt_evidence_from_tool_telemetry(tool_telemetry)
        .and_then(|value| value.get("delivery"))
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn automation_attempt_evidence_missing_capabilities(
    tool_telemetry: &Value,
) -> Vec<String> {
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

pub(crate) fn automation_capability_resolution_email_tools(
    tool_telemetry: &Value,
    key: &str,
) -> Vec<String> {
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

pub(crate) fn automation_capability_resolution_mcp_tools(
    tool_telemetry: &Value,
    key: &str,
) -> Vec<String> {
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

pub(crate) fn automation_capability_resolution_missing_capabilities(
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

pub(crate) fn automation_reset_attempt_tool_failure_labels(tool_telemetry: &mut Value) {
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

pub(crate) fn automation_initialized_attempt_tool_telemetry(
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

pub(crate) fn automation_normalize_server_list(raw: &[String]) -> Vec<String> {
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

pub(crate) fn automation_tool_names_for_mcp_server(
    tool_names: &[String],
    server_name: &str,
) -> Vec<String> {
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

pub(crate) fn automation_merge_mcp_capability_diagnostics(
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

pub(crate) fn automation_selected_mcp_servers_from_allowlist(
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

pub(crate) fn automation_selected_mcp_wildcard_servers_from_allowlist(
    allowlist: &[String],
    known_server_names: &[String],
) -> Vec<String> {
    let mut selected = Vec::new();
    for server_name in known_server_names {
        let namespace = crate::http::mcp::mcp_namespace_segment(server_name);
        if allowlist
            .iter()
            .any(|entry| entry.trim() == format!("mcp.{namespace}.*"))
        {
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

pub(crate) fn automation_add_mcp_list_when_scoped(
    mut requested_tools: Vec<String>,
    has_selected_mcp_servers: bool,
) -> Vec<String> {
    if has_selected_mcp_servers && !requested_tools.iter().any(|tool| tool == "mcp_list") {
        requested_tools.push("mcp_list".to_string());
    }
    requested_tools
}

pub(crate) fn automation_connector_hint_text(node: &AutomationFlowNode) -> String {
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

pub(crate) fn automation_tool_telemetry_selected_mcp_servers(
    tool_telemetry: &Value,
) -> Vec<String> {
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

pub(crate) fn automation_tool_telemetry_has_mcp_usage(tool_telemetry: &Value) -> bool {
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

pub(crate) fn automation_node_is_mcp_grounded_citations_artifact(
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

pub(crate) fn automation_text_mentions_mcp_server(text: &str, server_name: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let lowered_server = server_name.to_ascii_lowercase();
    let namespace = crate::http::mcp::mcp_namespace_segment(server_name);
    let mut needles = vec![
        lowered_server.clone(),
        lowered_server.replace('-', "_"),
        lowered_server.replace('-', " "),
        namespace.clone(),
        namespace.replace('_', "-"),
        format!("mcp.{namespace}"),
    ];
    for segment in lowered_server.split(['-', '_']) {
        let segment = segment.trim();
        if segment.len() >= 4 {
            needles.push(segment.to_string());
            needles.push(format!("{segment} mcp"));
        }
    }
    needles
        .iter()
        .filter(|needle| !needle.trim().is_empty())
        .any(|needle| lowered.contains(needle))
}

pub(crate) fn automation_requested_server_scoped_mcp_tools(
    node: &AutomationFlowNode,
    selected_server_names: &[String],
) -> Vec<String> {
    if automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ReviewDecision
    {
        return Vec::new();
    }
    let connector_hint_text = automation_connector_hint_text(node);
    if !tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
        &connector_hint_text,
    ) {
        return Vec::new();
    }
    if enforcement::automation_node_allows_optional_connector_references(node) {
        return Vec::new();
    }
    let concrete_tools = super::prompting_impl::automation_node_concrete_mcp_tool_allowlist(node);
    if !concrete_tools.is_empty() {
        return concrete_tools;
    }
    if super::node_runtime_impl::automation_node_has_explicit_tool_policy(node) {
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

pub(crate) fn automation_node_required_concrete_mcp_tools(
    node: &AutomationFlowNode,
) -> Vec<String> {
    if !automation_node_is_connector_preflight(node) {
        return Vec::new();
    }
    let mut tools = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("allowed_tools"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|tool| tool.starts_with("mcp.") && !tool.ends_with(".*"))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tools.sort();
    tools.dedup();
    tools
}

pub(crate) fn automation_node_is_connector_preflight(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .is_some_and(|builder| {
            ["task_class", "task_kind", "retry_class"]
                .iter()
                .filter_map(|key| builder.get(*key).and_then(Value::as_str))
                .any(|value| {
                    matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "connector_preflight" | "connector"
                    )
                })
        })
}

pub(crate) fn automation_node_required_tool_calls(
    node: &AutomationFlowNode,
) -> Vec<crate::AutomationRequiredToolCall> {
    let mut calls = Vec::new();
    if let Some(enforcement_calls) = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.enforcement.as_ref())
        .map(|enforcement| enforcement.required_tool_calls.clone())
    {
        calls.extend(enforcement_calls);
    }
    for source in [
        node.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("required_tool_calls")),
        node.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("required_tool_calls")),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(parsed) =
            serde_json::from_value::<Vec<crate::AutomationRequiredToolCall>>(source.clone())
        {
            calls.extend(parsed);
        }
    }
    if calls.is_empty() {
        calls.extend(
            automation_node_required_concrete_mcp_tools(node)
                .into_iter()
                .map(|tool| crate::AutomationRequiredToolCall {
                    tool,
                    args: None,
                    evidence_key: None,
                    required_success: true,
                }),
        );
    }
    let mut seen = std::collections::HashSet::new();
    calls
        .into_iter()
        .filter_map(|mut call| {
            call.tool = call.tool.trim().to_string();
            if call.tool.is_empty() {
                return None;
            }
            let dedupe_key = format!(
                "{}\n{}",
                call.tool,
                call.args.as_ref().map(Value::to_string).unwrap_or_default()
            );
            seen.insert(dedupe_key).then_some(call)
        })
        .collect()
}

async fn sync_automation_allowed_mcp_servers(
    state: &AppState,
    node: &AutomationFlowNode,
    allowed_servers: &[String],
    allowed_connections: &[crate::AutomationMcpConnectionGrant],
    allowlist: &[String],
    tenant_context: &TenantContext,
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
    let mut wildcard_selected_servers = automation_normalize_server_list(allowed_servers);
    wildcard_selected_servers.extend(automation_selected_mcp_wildcard_servers_from_allowlist(
        allowlist,
        &enabled_server_names,
    ));
    if selected_source == "email_fallback" {
        wildcard_selected_servers.extend(enabled_server_names.iter().cloned());
    }
    wildcard_selected_servers.sort();
    wildcard_selected_servers.dedup();
    if selected_servers.is_empty() {
        return json!({
            "selected_servers": [],
            "selected_source": "none",
            "wildcard_selected_servers": [],
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
        let connection_grant =
            automation_mcp_connection_grant_for_server(server_name, allowed_connections);
        let preflight_tenant_context =
            automation_mcp_preflight_tenant_context(tenant_context, connection_grant);
        let connected = if let (true, Ok(preflight_tenant_context)) =
            (enabled, preflight_tenant_context.as_ref())
        {
            // Single readiness gate (Invariant 2 of `docs/SPINE.md`):
            // 3 attempts with 0/750/1500ms delays, matching the previous
            // automation_connect_mcp_server_with_retry helper.
            state
                .mcp
                .ensure_ready_for_tenant(
                    server_name,
                    preflight_tenant_context,
                    tandem_runtime::mcp_ready::EnsureReadyPolicy::with_retries(3, 750),
                )
                .await
                .is_ok()
        } else {
            false
        };
        let sync_count = if let (true, Ok(preflight_tenant_context)) =
            (connected, preflight_tenant_context.as_ref())
        {
            crate::http::mcp::sync_mcp_tools_for_server_for_tenant(
                state,
                server_name,
                preflight_tenant_context,
            )
            .await as u64
        } else {
            0
        };
        let remote_names = if let (true, Ok(preflight_tenant_context)) =
            (connected, preflight_tenant_context.as_ref())
        {
            automation_mcp_remote_tool_names_for_tenant(
                state,
                server_name,
                preflight_tenant_context,
            )
            .await
        } else {
            Vec::new()
        };
        let sync_error = if !exists {
            Some("server_not_found")
        } else if !enabled {
            Some("server_disabled")
        } else if let Err(reason) = preflight_tenant_context.as_ref() {
            Some(*reason)
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
            "connection_grant": connection_grant,
            "preflight_tenant_context": preflight_tenant_context.as_ref().ok(),
            "registered_tool_count_after_sync": sync_count,
            "remote_tools": remote_names,
        }));
    }
    let mut missing_selected_servers = server_rows
        .iter()
        .filter(|row| row.get("sync_error").and_then(Value::as_str) == Some("server_not_found"))
        .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    missing_selected_servers.sort();
    missing_selected_servers.dedup();
    if selected_source == "policy" && !missing_selected_servers.is_empty() {
        wildcard_selected_servers.extend(enabled_server_names.iter().cloned());
        wildcard_selected_servers.sort();
        wildcard_selected_servers.dedup();
    }

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
        let remote_names = row
            .get("remote_tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();

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
        "missing_selected_servers": missing_selected_servers,
        "wildcard_selected_servers": wildcard_selected_servers,
        "servers": server_rows,
        "remote_tools": all_remote_names,
        "registered_tools": all_registered_names,
        "remote_email_like_tools": all_remote_email_like_names,
        "registered_email_like_tools": all_registered_email_like_names,
    })
}

pub(crate) fn automation_policy_mcp_preflight_blocker(diagnostics: &Value) -> Option<String> {
    if diagnostics
        .get("selected_source")
        .and_then(Value::as_str)
        .unwrap_or("none")
        != "policy"
    {
        return None;
    }
    let blocked = diagnostics
        .get("servers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let name = row.get("name").and_then(Value::as_str).unwrap_or("unknown");
            let connected = row
                .get("connected")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let registered_count = row
                .get("registered_tool_count_after_sync")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if connected && registered_count > 0 {
                None
            } else {
                let reason = row
                    .get("sync_error")
                    .and_then(Value::as_str)
                    .unwrap_or("no_registered_tools");
                if reason == "server_not_found" {
                    return None;
                }
                Some(format!("{name} ({reason})"))
            }
        })
        .collect::<Vec<_>>();
    if blocked.is_empty() {
        None
    } else {
        Some(format!(
            "required MCP server preflight failed before agent execution: {}",
            blocked.join(", ")
        ))
    }
}

#[cfg(test)]
mod automation_mcp_preflight_tests {
    use super::*;

    #[test]
    fn policy_mcp_preflight_blocks_disconnected_policy_server() {
        let diagnostics = json!({
            "selected_source": "policy",
            "servers": [{
                "name": "githubcopilot",
                "connected": false,
                "sync_error": "connect_failed",
                "registered_tool_count_after_sync": 0
            }]
        });
        let detail = automation_policy_mcp_preflight_blocker(&diagnostics).unwrap();
        assert!(detail.contains("githubcopilot"));
        assert!(detail.contains("connect_failed"));
    }

    #[test]
    fn policy_mcp_preflight_allows_stale_policy_server_name() {
        let diagnostics = json!({
            "selected_source": "policy",
            "servers": [{
                "name": "composio-1",
                "connected": false,
                "sync_error": "server_not_found",
                "registered_tool_count_after_sync": 0
            }]
        });
        assert!(automation_policy_mcp_preflight_blocker(&diagnostics).is_none());
    }

    #[test]
    fn policy_mcp_preflight_allows_connected_server_with_tools() {
        let diagnostics = json!({
            "selected_source": "policy",
            "servers": [{
                "name": "githubcopilot",
                "connected": true,
                "registered_tool_count_after_sync": 4
            }]
        });
        assert!(automation_policy_mcp_preflight_blocker(&diagnostics).is_none());
    }

    #[test]
    fn policy_mcp_preflight_ignores_non_policy_selection() {
        let diagnostics = json!({
            "selected_source": "none",
            "servers": [{
                "name": "githubcopilot",
                "connected": false,
                "registered_tool_count_after_sync": 0
            }]
        });
        assert!(automation_policy_mcp_preflight_blocker(&diagnostics).is_none());
    }
}

pub(crate) fn automation_node_delivery_method_value(node: &AutomationFlowNode) -> String {
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
        metadata: node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("approval"))
            .and_then(|approval| approval.get("metadata"))
            .cloned(),
        expiry_policy: gate
            .expiry_policy
            .clone()
            .or_else(|| automation_gate_expiry_policy_from_node_metadata(node)),
    })
}

fn automation_gate_expiry_policy_from_node_metadata(
    node: &AutomationFlowNode,
) -> Option<crate::automation_v2::types::AutomationGateExpiryPolicy> {
    let approval = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("approval"))?;
    [
        "/expiry_policy",
        "/gate_expiry_policy",
        "/policy/expiry",
        "/policy/gate_expiry",
    ]
    .iter()
    .find_map(|pointer| approval.pointer(pointer))
    .cloned()
    .and_then(|value| serde_json::from_value(value).ok())
}

pub(crate) fn truncate_path_list_for_prompt(paths: Vec<String>, limit: usize) -> Vec<String> {
    let mut deduped = normalize_non_empty_list(paths);
    if deduped.len() > limit {
        deduped.truncate(limit);
    }
    deduped
}

pub(crate) fn value_object_path_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

pub(crate) fn render_research_finalize_upstream_summary(
    upstream_inputs: &[Value],
) -> Option<String> {
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

pub(crate) fn render_upstream_synthesis_guidance(
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

pub(crate) fn automation_prompt_html_escape(text: &str) -> String {
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

pub(crate) fn automation_prompt_canonicalize_artifact_hrefs(text: &str, run_id: &str) -> String {
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

pub(crate) fn automation_prompt_render_canonical_html_body(text: &str, run_id: &str) -> String {
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

pub(crate) fn render_deterministic_delivery_body(
    upstream_inputs: &[Value],
    run_id: &str,
) -> Option<String> {
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

pub(crate) fn automation_phase_execution_mode_map(
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

pub(crate) fn automation_plan_package(
    automation: &AutomationV2Spec,
) -> Option<compiler_api::PlanPackage> {
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

pub(crate) fn normalize_write_scope_entries(scope: Option<String>) -> Vec<String> {
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

pub(crate) fn write_scope_entries_conflict(left: &[String], right: &[String]) -> bool {
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
    let mut selected_mcp_tools = Vec::<Vec<String>>::new();
    for node in runnable {
        let is_code = automation_node_is_code_workflow(&node);
        let scope_entries = if is_code {
            normalize_write_scope_entries(automation_node_write_scope(&node))
        } else {
            Vec::new()
        };
        let mcp_tool_entries = automation_node_explicit_mcp_tool_entries(&node);
        let conflicts = is_code
            && selected.iter().enumerate().any(|(index, existing)| {
                automation_node_is_code_workflow(existing)
                    && write_scope_entries_conflict(&scope_entries, &selected_scopes[index])
            });
        let mcp_conflicts = !mcp_tool_entries.is_empty()
            && selected_mcp_tools.iter().any(|existing| {
                existing
                    .iter()
                    .any(|tool| mcp_tool_entries.iter().any(|candidate| candidate == tool))
            });
        if conflicts || mcp_conflicts {
            continue;
        }
        if is_code {
            selected_scopes.push(scope_entries);
        } else {
            selected_scopes.push(Vec::new());
        }
        selected_mcp_tools.push(mcp_tool_entries);
        selected.push(node);
        if selected.len() >= max_parallel {
            break;
        }
    }
    selected
}

fn automation_node_explicit_mcp_tool_entries(node: &AutomationFlowNode) -> Vec<String> {
    let mut text = node.objective.clone();
    if let Some(metadata) = node.metadata.as_ref() {
        text.push(' ');
        text.push_str(&metadata.to_string());
    }
    let mut tools = text
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '*')))
        .map(str::trim)
        .filter(|token| token.starts_with("mcp."))
        .filter(|token| !token.ends_with(".*"))
        .filter(|token| token.split('.').count() >= 3)
        .map(|token| token.trim_matches('.').to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tools.sort();
    tools.dedup();
    tools
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

pub(crate) fn automation_mission_milestones(automation: &AutomationV2Spec) -> Vec<Value> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("milestones"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn completed_mission_milestones(
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
