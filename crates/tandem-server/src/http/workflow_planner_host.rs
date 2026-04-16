use serde_json::Value;
use std::collections::BTreeSet;
use tandem_plan_compiler::api as compiler_api;
use tandem_types::{Message, MessagePart, MessageRole};

use super::*;
use compiler_api::{
    prepare_build_request, Clock, McpToolCatalog, PlanStore, PlannerBuildConfig,
    PlannerBuildResult, PlannerInvocationFailure, PlannerLlmInvocation, PlannerLlmInvoker,
    PlannerModelRegistry, PlannerSessionStore, TelemetrySink, WorkspaceResolver,
};

pub(crate) struct WorkflowPlannerHost<'a> {
    pub(crate) state: &'a AppState,
}

pub(crate) async fn resolve_workspace_root(
    state: &AppState,
    requested: Option<&str>,
) -> Result<String, String> {
    let root = state.workspace_index.snapshot().await.root;
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    compiler_api::resolve_workspace_root_candidate(requested, &root, cwd.as_deref())
}

fn workflow_step_contract_kind(step: &crate::WorkflowPlanStep) -> String {
    step.output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

fn workflow_step_is_upstream_synthesis(step: &crate::WorkflowPlanStep) -> bool {
    let has_upstream_dependencies = !step.depends_on.is_empty() || !step.input_refs.is_empty();
    if !has_upstream_dependencies {
        return false;
    }
    let contract_kind = workflow_step_contract_kind(step);
    if !matches!(
        contract_kind.as_str(),
        "structured_json" | "report_markdown" | "text_summary" | "review_summary"
    ) {
        return false;
    }
    let lowered = format!(
        "{}\n{}\n{}",
        step.step_id.to_ascii_lowercase(),
        step.kind.to_ascii_lowercase(),
        step.objective.to_ascii_lowercase()
    );
    [
        "summar",
        "synthes",
        "report",
        "final",
        "finalize",
        "deliverable",
        "append",
        "merge",
        "consolidat",
        "recap",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn workflow_step_alias_from_step_id(step_id: &str) -> String {
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

fn strengthen_upstream_synthesis_step(step: &mut crate::WorkflowPlanStep) {
    if !workflow_step_is_upstream_synthesis(step) {
        return;
    }

    let mut upstream_step_ids = BTreeSet::new();
    for dep in &step.depends_on {
        let trimmed = dep.trim();
        if !trimmed.is_empty() {
            upstream_step_ids.insert(trimmed.to_string());
        }
    }
    for input_ref in &step.input_refs {
        let trimmed = input_ref.from_step_id.trim();
        if !trimmed.is_empty() {
            upstream_step_ids.insert(trimmed.to_string());
        }
    }
    if upstream_step_ids.is_empty() {
        return;
    }

    let mut existing_aliases = step
        .input_refs
        .iter()
        .map(|input_ref| input_ref.alias.trim().to_string())
        .collect::<BTreeSet<_>>();
    let existing_inputs = step
        .input_refs
        .iter()
        .map(|input_ref| input_ref.from_step_id.trim().to_string())
        .collect::<BTreeSet<_>>();
    for upstream_step_id in &upstream_step_ids {
        if existing_inputs.contains(upstream_step_id) {
            continue;
        }
        let alias_base = workflow_step_alias_from_step_id(upstream_step_id);
        let mut alias = alias_base.clone();
        let mut index = 2u32;
        while existing_aliases.contains(&alias) {
            alias = format!("{alias_base}_{index}");
            index += 1;
        }
        existing_aliases.insert(alias.clone());
        step.input_refs.push(crate::AutomationFlowInputRef {
            from_step_id: upstream_step_id.clone(),
            alias,
        });
    }

    let upstream_summary = upstream_step_ids
        .iter()
        .take(4)
        .map(|step_id| format!("`{step_id}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let synthesis_guidance = format!(
        "Read and synthesize the strongest upstream artifacts from {}. Reuse the concrete filenames, named entities, URLs, counts, match reasons, risks, and proof points from those upstream steps instead of producing a generic recap.",
        upstream_summary
    );

    if let Some(contract) = step.output_contract.as_mut() {
        contract.summary_guidance = match contract.summary_guidance.take() {
            Some(existing)
                if existing
                    .to_ascii_lowercase()
                    .contains("read and synthesize the strongest upstream artifacts") =>
            {
                Some(existing)
            }
            Some(existing) if !existing.trim().is_empty() => {
                Some(format!("{} {}", existing.trim(), synthesis_guidance))
            }
            _ => Some(synthesis_guidance.clone()),
        };
    }

    let metadata = step.metadata.get_or_insert_with(|| serde_json::json!({}));
    if let Some(builder) = metadata
        .as_object_mut()
        .map(|root| {
            root.entry("builder".to_string())
                .or_insert_with(|| serde_json::json!({}))
        })
        .and_then(Value::as_object_mut)
    {
        builder.insert(
            "upstream_input_step_ids".to_string(),
            serde_json::json!(upstream_step_ids.iter().cloned().collect::<Vec<_>>()),
        );
        let strengthened_prompt = format!(
            "Before writing the final artifact, read and synthesize the strongest upstream artifacts from {}. Carry forward concrete evidence anchors from those steps and do not collapse the result into a vague recap.",
            upstream_summary
        );
        match builder.get("prompt").and_then(Value::as_str).map(str::trim) {
            Some(existing)
                if existing
                    .to_ascii_lowercase()
                    .contains("read and synthesize the strongest upstream artifacts") => {}
            Some(existing) if !existing.is_empty() => {
                builder.insert(
                    "prompt".to_string(),
                    Value::String(format!("{existing} {strengthened_prompt}")),
                );
            }
            _ => {
                builder.insert("prompt".to_string(), Value::String(strengthened_prompt));
            }
        }
    }
}

pub(crate) fn normalize_workflow_step_metadata(step: &mut crate::WorkflowPlanStep) {
    compiler_api::normalize_workflow_step_metadata(
        step,
        |step| step.step_id.as_str(),
        |step| step.kind.as_str(),
        |step| step.objective.as_str(),
        |step| {
            step.output_contract
                .as_ref()
                .map(|contract| {
                    compiler_api::output_contract_is_research_brief(
                        &contract.kind,
                        contract.validator.map(|kind| kind.stable_key()),
                    )
                })
                .unwrap_or(false)
        },
        |step| {
            step.output_contract
                .as_ref()
                .map(|contract| contract.enforcement.is_none())
                .unwrap_or(true)
        },
        |step, value| {
            if let Some(contract) = step.output_contract.as_mut() {
                if contract.enforcement.is_none() {
                    if let Ok(enforcement) = serde_json::from_value(value) {
                        contract.enforcement = Some(enforcement);
                    }
                }
            }
        },
        |step| step.metadata.as_ref(),
        |step, value| {
            step.metadata = Some(value);
        },
    );
    strengthen_upstream_synthesis_step(step);
}

pub(crate) fn normalize_workflow_plan_file_contracts(plan: &mut crate::WorkflowPlan) {
    compiler_api::derive_workflow_step_file_contracts(plan);
}

pub(crate) async fn build_workflow_plan(
    state: &AppState,
    prompt: &str,
    explicit_schedule: Option<&Value>,
    plan_source: &str,
    allowed_mcp_servers: Vec<String>,
    workspace_root: Option<&str>,
    operator_preferences: Option<Value>,
) -> Result<
    PlannerBuildResult<
        crate::routines::types::RoutineMisfirePolicy,
        crate::AutomationFlowInputRef,
        crate::AutomationFlowOutputContract,
    >,
    String,
> {
    let plan_id = format!("wfplan-{}", uuid::Uuid::new_v4());
    let planner_version = "v1".to_string();

    let host = WorkflowPlannerHost { state };
    let request = prepare_build_request(
        plan_id,
        planner_version,
        plan_source.to_string(),
        prompt,
        explicit_schedule,
        "UTC",
        crate::RoutineMisfirePolicy::RunOnce,
        allowed_mcp_servers,
        workspace_root,
        operator_preferences,
    );

    let mut result = compiler_api::build_workflow_plan_with_planner(
        &host,
        request,
        PlannerBuildConfig {
            session_title: "Workflow Planner Create".to_string(),
            timeout_ms: super::workflow_planner_policy::planner_build_timeout_ms(),
            override_env: "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE".to_string(),
        },
        normalize_workflow_step_metadata,
        compiler_api::plan_step_with_dep::<
            crate::AutomationFlowInputRef,
            crate::AutomationFlowOutputContract,
        >(
            "execute_goal",
            "execute",
            "Execute the requested automation goal directly.",
            "worker",
            &[] as &[String],
            Vec::new(),
            Some(compiler_api::default_execute_goal_output_contract_seed().into()),
            None,
        ),
    )
    .await;
    normalize_workflow_plan_file_contracts(&mut result.plan);

    Ok(result)
}

#[async_trait::async_trait]
impl<'a> PlannerModelRegistry for WorkflowPlannerHost<'a> {
    async fn is_provider_configured(&self, provider_id: &str) -> bool {
        self.state
            .providers
            .list()
            .await
            .into_iter()
            .any(|provider| provider.id == provider_id)
    }
}

#[async_trait::async_trait]
impl<'a> PlanStore for WorkflowPlannerHost<'a> {
    async fn get_draft(&self, plan_id: &str) -> Result<Option<Value>, String> {
        self.state
            .get_workflow_plan_draft(plan_id)
            .await
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| truncate_text(&error.to_string(), 500))
    }

    async fn put_draft(&self, _plan_id: &str, draft: Value) -> Result<(), String> {
        let draft: crate::WorkflowPlanDraftRecord = serde_json::from_value(draft)
            .map_err(|error| truncate_text(&error.to_string(), 500))?;
        self.state.put_workflow_plan_draft(draft).await;
        Ok(())
    }
}

#[async_trait::async_trait]
impl<'a> McpToolCatalog for WorkflowPlannerHost<'a> {
    async fn capability_summary(&self, allowed_mcp_servers: &[String]) -> Value {
        build_planner_capability_summary(self.state, allowed_mcp_servers).await
    }
}

#[async_trait::async_trait]
impl<'a> PlannerLlmInvoker for WorkflowPlannerHost<'a> {
    async fn invoke_planner_llm(
        &self,
        invocation: PlannerLlmInvocation,
    ) -> Result<Value, PlannerInvocationFailure> {
        invoke_planner_llm(
            self.state,
            &invocation.session_title,
            &invocation.workspace_root,
            invocation.model,
            invocation.prompt,
            invocation.run_key,
            invocation.timeout_ms,
            &invocation.override_env,
        )
        .await
    }
}

#[async_trait::async_trait]
impl<'a> WorkspaceResolver for WorkflowPlannerHost<'a> {
    async fn resolve_workspace_root(&self, requested: Option<&str>) -> Result<String, String> {
        resolve_workspace_root(self.state, requested).await
    }
}

impl<'a> TelemetrySink for WorkflowPlannerHost<'a> {
    fn warn(&self, message: &str) {
        tracing::warn!("{message}");
    }
}

impl<'a> Clock for WorkflowPlannerHost<'a> {
    fn now_ms(&self) -> u64 {
        crate::now_ms()
    }
}

#[async_trait::async_trait]
impl<'a> PlannerSessionStore for WorkflowPlannerHost<'a> {
    async fn create_planner_session(
        &self,
        title: &str,
        workspace_root: &str,
    ) -> Result<String, String> {
        let mut session = Session::new(Some(title.to_string()), Some(workspace_root.to_string()));
        let session_id = session.id.clone();
        session.workspace_root = Some(workspace_root.to_string());
        self.state
            .storage
            .save_session(session)
            .await
            .map_err(|error| truncate_text(&error.to_string(), 500))?;
        Ok(session_id)
    }

    async fn append_planner_user_prompt(
        &self,
        session_id: &str,
        prompt: &str,
    ) -> Result<(), String> {
        self.state
            .storage
            .append_message(
                session_id,
                Message::new(
                    MessageRole::User,
                    vec![MessagePart::Text {
                        text: prompt.to_string(),
                    }],
                ),
            )
            .await
            .map_err(|error| truncate_text(&error.to_string(), 500))
    }

    async fn append_planner_assistant_response(
        &self,
        session_id: &str,
        response: &str,
    ) -> Result<(), String> {
        self.state
            .storage
            .append_message(
                session_id,
                Message::new(
                    MessageRole::Assistant,
                    vec![MessagePart::Text {
                        text: response.to_string(),
                    }],
                ),
            )
            .await
            .map_err(|error| truncate_text(&error.to_string(), 500))
    }
}

async fn invoke_planner_llm(
    state: &AppState,
    session_title: &str,
    workspace_root: &str,
    model: tandem_types::ModelSpec,
    prompt: String,
    _run_key: String,
    timeout_ms: u64,
    override_env: &str,
) -> Result<Value, PlannerInvocationFailure> {
    if let Some(payload) =
        super::workflow_planner_policy::planner_test_override_payload(override_env, true)
    {
        return Ok(payload);
    }
    let host = WorkflowPlannerHost { state };
    let original_prompt = prompt.clone();
    let workspace_root = resolve_workspace_root(state, Some(workspace_root))
        .await
        .map_err(|error| PlannerInvocationFailure {
            reason: "invalid_workspace_root".to_string(),
            detail: Some(error),
        })?;
    let session_id =
        compiler_api::begin_planner_session(&host, session_title, &workspace_root, &prompt).await?;
    let output = super::workflow_planner_transport::invoke_planner_provider(
        state,
        &session_id,
        &model,
        prompt,
        timeout_ms,
    )
    .await?;

    if output.trim().is_empty() {
        return Err(PlannerInvocationFailure {
            reason: "empty_output".to_string(),
            detail: Some("Workflow planner completed without assistant text.".to_string()),
        });
    }
    if let Some(value) = extract_planner_json_value(&output) {
        compiler_api::finish_planner_session(&host, &session_id, &output).await?;
        return Ok(value);
    }

    tracing::warn!(
        "workflow planner returned non-JSON text; requesting a JSON-only repair response"
    );
    compiler_api::finish_planner_session(&host, &session_id, &output).await?;
    let repair_prompt = build_planner_json_repair_prompt(session_title, &original_prompt, &output);
    compiler_api::PlannerSessionStore::append_planner_user_prompt(
        &host,
        &session_id,
        &repair_prompt,
    )
    .await
    .map_err(|error| PlannerInvocationFailure {
        reason: "storage_error".to_string(),
        detail: Some(truncate_text(&error, 500)),
    })?;
    let repair_output = super::workflow_planner_transport::invoke_planner_provider(
        state,
        &session_id,
        &model,
        repair_prompt,
        timeout_ms,
    )
    .await?;
    compiler_api::finish_planner_session(&host, &session_id, &repair_output).await?;
    extract_planner_json_value(&repair_output).ok_or_else(|| PlannerInvocationFailure {
        reason: "invalid_json".to_string(),
        detail: Some(
            "Workflow planner returned text without valid JSON, including after a repair retry."
                .to_string(),
        ),
    })
}

async fn build_planner_capability_summary(
    state: &AppState,
    allowed_mcp_servers: &[String],
) -> Value {
    if allowed_mcp_servers.is_empty() {
        let mut summary = compiler_api::build_planner_capability_summary(&[]);
        if let Some(object) = summary.as_object_mut() {
            object.insert(
                "mcp_inventory".to_string(),
                json!({
                    "connected_server_names": [],
                    "enabled_server_names": [],
                    "inventory_version": 1,
                    "registered_tools": [],
                    "remote_tools": [],
                    "servers": [],
                }),
            );
            object.insert(
                "mcp_inventory_source".to_string(),
                Value::String("allowlist_empty".to_string()),
            );
        }
        return summary;
    }

    let (mcp_inventory, mcp_inventory_source) = planner_mcp_inventory_snapshot(state).await;
    let mut server_tools = Vec::new();
    for server in allowed_mcp_servers {
        let tools = state.mcp.server_tools(server).await;
        let mut tool_names = Vec::new();
        for tool in tools.iter() {
            let tool_name = tool.namespaced_name.trim().to_string();
            if !tool_name.is_empty() {
                tool_names.push(tool_name);
            }
        }
        server_tools.push(compiler_api::PlannerMcpServerToolSet {
            server: server.to_string(),
            tool_names,
        });
    }
    let filtered_inventory = filter_mcp_inventory_to_allowed(mcp_inventory, allowed_mcp_servers);
    let mut summary = compiler_api::build_planner_capability_summary(&server_tools);
    if let Some(object) = summary.as_object_mut() {
        object.insert("mcp_inventory".to_string(), filtered_inventory);
        object.insert(
            "mcp_inventory_source".to_string(),
            Value::String(mcp_inventory_source.to_string()),
        );
    }
    summary
}

async fn planner_mcp_inventory_snapshot(state: &AppState) -> (Value, &'static str) {
    match state.tools.execute("mcp_list", json!({})).await {
        Ok(result) => {
            if let Some(metadata) = result.metadata.as_object().cloned().map(Value::Object) {
                return (metadata, "mcp_list");
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(result.output.trim()) {
                return (parsed, "mcp_list");
            }
            tracing::warn!(
                "mcp_list tool returned an unparseable inventory; falling back to direct snapshot"
            );
        }
        Err(error) => {
            tracing::warn!(error = %error, "planner preflight mcp_list invocation failed");
        }
    }
    (
        crate::http::mcp::mcp_inventory_snapshot(state).await,
        "runtime_snapshot",
    )
}

fn build_planner_json_repair_prompt(
    session_title: &str,
    original_prompt: &str,
    invalid_output: &str,
) -> String {
    let prompt = original_prompt.trim();
    let output = invalid_output.trim();
    format!(
        concat!(
            "You are revising a Tandem automation workflow plan.\n",
            "Planner intelligence lives in the model. Return JSON only.\n",
            "The previous planner response was not valid JSON.\n",
            "Return one valid JSON object that matches the planner contract exactly.\n",
            "Do not add markdown fences, prose, explanations, or commentary.\n",
            "Session title: {}\n",
            "Original prompt:\n{}\n\n",
            "Invalid planner response to repair:\n{}\n"
        ),
        session_title.trim(),
        prompt,
        output
    )
}

fn extract_planner_json_value(text: &str) -> Option<Value> {
    compiler_api::extract_json_value_from_text(text)
        .map(normalize_planner_payload_aliases)
        .or_else(|| salvage_planner_payload_from_text(text))
}

fn normalize_planner_payload_aliases(mut payload: Value) -> Value {
    if let Some(object) = payload.as_object_mut() {
        if !object.contains_key("plan") {
            if let Some(workflow_plan) = object.remove("workflow_plan") {
                object.insert("plan".to_string(), workflow_plan);
            }
        }
    }
    payload
}

fn salvage_planner_payload_from_text(text: &str) -> Option<Value> {
    let action = parse_json_string_field(text, "action")?;
    let mut payload = serde_json::Map::new();
    payload.insert("action".to_string(), Value::String(action.clone()));

    if let Some(plan) = parse_json_value_field(text, "plan")
        .or_else(|| parse_json_value_field(text, "workflow_plan"))
    {
        payload.insert("plan".to_string(), plan);
    }
    if let Some(assistant_text) = parse_json_string_field(text, "assistant_text") {
        if !assistant_text.trim().is_empty() {
            payload.insert("assistant_text".to_string(), Value::String(assistant_text));
        }
    }
    if let Some(change_summary) = parse_json_value_field(text, "change_summary") {
        payload.insert("change_summary".to_string(), change_summary);
    }
    if let Some(clarifier) = parse_json_value_field(text, "clarifier") {
        payload.insert("clarifier".to_string(), clarifier);
    }

    match action.as_str() {
        "build" | "revise" => {
            if !payload.contains_key("plan") {
                return None;
            }
        }
        "clarify" => {
            if !payload.contains_key("clarifier") {
                return None;
            }
        }
        "keep" => {}
        _ => return None,
    }

    Some(Value::Object(payload))
}

fn parse_json_value_field(text: &str, key: &str) -> Option<Value> {
    let start = find_json_field_value_start(text, key)?;
    let first = text[start..].chars().next()?;
    if first != '{' && first != '[' {
        return None;
    }
    let fragment = extract_balanced_json_fragment_at(text, start)?;
    serde_json::from_str::<Value>(fragment).ok()
}

fn parse_json_string_field(text: &str, key: &str) -> Option<String> {
    let start = find_json_field_value_start(text, key)?;
    let rest = &text[start..];
    let mut chars = rest.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escape = false;
    for ch in chars {
        if escape {
            out.push(ch);
            escape = false;
            continue;
        }
        match ch {
            '\\' => escape = true,
            '"' => return Some(out),
            _ => out.push(ch),
        }
    }
    None
}

fn find_json_field_value_start(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let mut search_from = 0usize;
    while let Some(relative) = text[search_from..].find(&needle) {
        let key_start = search_from + relative;
        let mut idx = key_start + needle.len();
        while let Some(ch) = text[idx..].chars().next() {
            if ch.is_whitespace() {
                idx += ch.len_utf8();
                continue;
            }
            if ch == ':' {
                idx += ch.len_utf8();
                break;
            }
            idx = key_start + 1;
            break;
        }
        if idx <= key_start + needle.len() {
            search_from = key_start + 1;
            continue;
        }
        while let Some(ch) = text[idx..].chars().next() {
            if ch.is_whitespace() {
                idx += ch.len_utf8();
            } else {
                break;
            }
        }
        return Some(idx);
    }
    None
}

fn extract_balanced_json_fragment_at(text: &str, start: usize) -> Option<&str> {
    let slice = text.get(start..)?;
    let mut iter = slice.char_indices();
    let (_, first) = iter.next()?;
    if first != '{' && first != '[' {
        return None;
    }

    let mut stack = vec![first];
    let mut in_string = false;
    let mut escape = false;
    for (offset, ch) in iter {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => stack.push(ch),
            '}' | ']' => {
                let open = stack.pop()?;
                if !matches!((open, ch), ('{', '}') | ('[', ']')) {
                    return None;
                }
                if stack.is_empty() {
                    let end = start + offset + ch.len_utf8();
                    return text.get(start..end);
                }
            }
            _ => {}
        }
    }
    None
}

fn filter_mcp_inventory_to_allowed(inventory: Value, allowed_mcp_servers: &[String]) -> Value {
    let allowed = allowed_mcp_servers
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>();
    let filtered_servers = inventory
        .get("servers")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|row| {
                    row.get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .map(|name| allowed.contains(name))
                        .unwrap_or(false)
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut connected_server_names = Vec::new();
    let mut enabled_server_names = Vec::new();
    let mut registered_tools = BTreeSet::new();
    let mut remote_tools = BTreeSet::new();
    for server in filtered_servers.iter() {
        let Some(name) = server.get("name").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if server
            .get("connected")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            connected_server_names.push(name.to_string());
        }
        if server
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            enabled_server_names.push(name.to_string());
        }
        for tool_name in server
            .get("registered_tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            registered_tools.insert(tool_name.to_string());
        }
        for tool_name in server
            .get("remote_tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            remote_tools.insert(tool_name.to_string());
        }
    }

    json!({
        "connected_server_names": connected_server_names,
        "enabled_server_names": enabled_server_names,
        "inventory_version": inventory.get("inventory_version").and_then(Value::as_u64).unwrap_or(1),
        "registered_tools": registered_tools.into_iter().collect::<Vec<_>>(),
        "remote_tools": remote_tools.into_iter().collect::<Vec<_>>(),
        "servers": filtered_servers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn planner_capability_summary_skips_inventory_when_allowlist_is_empty() {
        let state = crate::http::tests::test_state().await;
        let summary = build_planner_capability_summary(&state, &[]).await;
        let inventory = summary
            .get("mcp_inventory")
            .and_then(Value::as_object)
            .expect("mcp inventory");
        assert_eq!(
            summary.get("mcp_inventory_source").and_then(Value::as_str),
            Some("allowlist_empty")
        );
        let servers = inventory
            .get("servers")
            .and_then(Value::as_array)
            .expect("servers");
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn planner_capability_summary_filters_inventory_to_allowlist() {
        let state = crate::http::tests::test_state().await;
        let summary = build_planner_capability_summary(&state, &["github".to_string()]).await;
        let inventory = summary
            .get("mcp_inventory")
            .and_then(Value::as_object)
            .expect("mcp inventory");
        assert_eq!(
            summary.get("mcp_inventory_source").and_then(Value::as_str),
            Some("mcp_list")
        );
        let servers = inventory
            .get("servers")
            .and_then(Value::as_array)
            .expect("servers");
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers[0].get("name").and_then(Value::as_str),
            Some("github")
        );
    }

    #[test]
    fn extract_planner_json_value_salvages_workflow_plan_alias() {
        let text = r#"{"action":"build","assistant_text":"ok","workflow_plan":{"title":"Demo","steps":[]}}"#;
        let value = extract_planner_json_value(text).expect("value");
        assert_eq!(value.get("action").and_then(Value::as_str), Some("build"));
        assert!(value.get("plan").is_some());
    }

    #[test]
    fn extract_planner_json_value_salvages_malformed_assistant_text() {
        let text = r#"{"action":"build","assistant_text":"{"plan":{"title":"Demo","steps":[]}}","plan":{"title":"Demo","steps":[]}}"#;
        let value = extract_planner_json_value(text).expect("value");
        assert_eq!(value.get("action").and_then(Value::as_str), Some("build"));
        assert_eq!(
            value
                .get("plan")
                .and_then(|plan| plan.get("title"))
                .and_then(Value::as_str),
            Some("Demo")
        );
    }

    #[test]
    fn planner_json_repair_prompt_requests_json_only() {
        let prompt = build_planner_json_repair_prompt(
            "Workflow Planner Create",
            "Build a workflow",
            "Here is the plan: research, analyze, report.",
        );
        assert!(prompt.contains("Return JSON only"));
        assert!(prompt.contains("previous planner response was not valid JSON"));
        assert!(prompt.contains("Invalid planner response to repair"));
        assert!(prompt.contains("Build a workflow"));
    }

    fn synthesis_step() -> crate::WorkflowPlanStep {
        crate::WorkflowPlanStep {
            step_id: "summarize_search_run".to_string(),
            kind: "summarize".to_string(),
            objective: "Summarize the search run into a final results artifact.".to_string(),
            agent_role: "writer".to_string(),
            depends_on: vec![
                "extract_job_board_sources".to_string(),
                "detect_repeated_listings".to_string(),
            ],
            input_refs: Vec::new(),
            output_contract: Some(crate::AutomationFlowOutputContract {
                kind: "structured_json".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                enforcement: None,
                schema: None,
                summary_guidance: None,
            }),
            metadata: None,
        }
    }

    #[test]
    fn normalize_workflow_step_metadata_strengthens_upstream_synthesis_steps() {
        let mut step = synthesis_step();

        normalize_workflow_step_metadata(&mut step);

        assert_eq!(step.input_refs.len(), 2);
        assert_eq!(step.input_refs[0].from_step_id, "extract_job_board_sources");
        assert_eq!(step.input_refs[1].from_step_id, "detect_repeated_listings");
        assert!(step
            .output_contract
            .as_ref()
            .and_then(|contract| contract.summary_guidance.as_deref())
            .is_some_and(|guidance| guidance
                .contains("Read and synthesize the strongest upstream artifacts")));
        assert!(step
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.pointer("/builder/upstream_input_step_ids"))
            .and_then(Value::as_array)
            .is_some_and(|items| items.len() == 2));
        assert!(
            step.metadata
                .as_ref()
                .and_then(|metadata| metadata.pointer("/builder/prompt"))
                .and_then(Value::as_str)
                .is_some_and(
                    |prompt| prompt.contains("do not collapse the result into a vague recap")
                )
        );
    }

    #[test]
    fn normalize_workflow_step_metadata_leaves_non_synthesis_steps_without_fabricated_inputs() {
        let mut step = crate::WorkflowPlanStep {
            step_id: "score_listing_relevance".to_string(),
            kind: "score".to_string(),
            objective: "Score listing relevance.".to_string(),
            agent_role: "analyst".to_string(),
            depends_on: vec!["extract_listing_candidates".to_string()],
            input_refs: Vec::new(),
            output_contract: Some(crate::AutomationFlowOutputContract {
                kind: "structured_json".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
                enforcement: None,
                schema: Some(json!({"type": "object"})),
                summary_guidance: None,
            }),
            metadata: None,
        };

        normalize_workflow_step_metadata(&mut step);

        assert!(step.input_refs.is_empty());
        assert!(step
            .output_contract
            .as_ref()
            .and_then(|contract| contract.summary_guidance.as_deref())
            .is_none());
    }
}
