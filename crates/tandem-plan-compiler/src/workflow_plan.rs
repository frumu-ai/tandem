// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tandem_workflows::plan_package::{
    AutomationV2Schedule, AutomationV2ScheduleType, WorkflowPlan, WorkflowPlanConversation,
    WorkflowPlanDraftRecord, WorkflowPlanStep,
};

use crate::contracts::{research_output_contract_policy_seed, ProjectedOutputValidatorKind};
use crate::decomposition::{
    derive_step_decomposition_hints, derive_workflow_decomposition_profile,
};

pub fn plan_save_options() -> Value {
    json!({
        "can_export_pack": true,
        "can_save_skill": true
    })
}

#[derive(Debug, Clone)]
pub struct PackBuilderExportOptions {
    pub session_id: Option<String>,
    pub thread_key: Option<String>,
    pub auto_apply: bool,
}

pub fn pack_builder_schedule_value<M>(schedule: &AutomationV2Schedule<M>) -> Value {
    match schedule.schedule_type {
        AutomationV2ScheduleType::Cron => schedule
            .cron_expression
            .as_ref()
            .map(|expression| {
                json!({
                    "cron": expression,
                    "timezone": schedule.timezone,
                })
            })
            .unwrap_or(Value::Null),
        AutomationV2ScheduleType::Interval => json!({
            "interval_seconds": schedule.interval_seconds.unwrap_or(86_400),
            "timezone": schedule.timezone,
        }),
        AutomationV2ScheduleType::Manual => Value::Null,
    }
}

pub fn pack_builder_export_goal<M, S>(plan: &WorkflowPlan<AutomationV2Schedule<M>, S>) -> String {
    let goal = plan
        .original_prompt
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if goal.is_empty() {
        plan.title.clone()
    } else {
        goal
    }
}

pub fn pack_builder_export_args<M, S>(
    plan: &WorkflowPlan<AutomationV2Schedule<M>, S>,
    options: &PackBuilderExportOptions,
) -> Value {
    json!({
        "mode": "preview",
        "goal": pack_builder_export_goal(plan),
        "__session_id": options.session_id,
        "thread_key": options.thread_key,
        "auto_apply": options.auto_apply,
        "schedule": pack_builder_schedule_value(&plan.schedule),
    })
}

pub fn workflow_plan_draft_record<Plan: Clone>(
    plan: Plan,
    plan_id: String,
    planner_diagnostics: Option<Value>,
    conversation_id: String,
    created_at_ms: u64,
) -> WorkflowPlanDraftRecord<Plan> {
    WorkflowPlanDraftRecord {
        current_plan: plan.clone(),
        plan_revision: 1,
        conversation: WorkflowPlanConversation {
            conversation_id,
            plan_id,
            created_at_ms,
            updated_at_ms: created_at_ms,
            messages: Vec::new(),
        },
        planner_diagnostics,
        last_success_materialization: None,
        initial_plan: plan,
    }
}

pub fn workflow_step_expects_web_research(step_id: &str, kind: &str, objective: &str) -> bool {
    let lowered_step_id = step_id.trim().to_ascii_lowercase();
    let lowered_kind = kind.trim().to_ascii_lowercase();
    let lowered_objective = objective.trim().to_ascii_lowercase();
    lowered_step_id.contains("research")
        || lowered_kind.contains("research")
        || lowered_objective.contains("web")
        || lowered_objective.contains("online")
        || lowered_objective.contains("current")
        || lowered_objective.contains("latest")
}

pub fn workflow_step_metadata_defaults(
    step_id: &str,
    kind: &str,
    objective: &str,
    validator_is_research_brief: bool,
) -> Option<Value> {
    let expects_web_research = workflow_step_expects_web_research(step_id, kind, objective);
    let mut builder = json!({
        "builder": {
            "knowledge": {
                "enabled": true,
                "reuse_mode": "preflight",
                "trust_floor": "promoted",
                "read_spaces": [{"scope": "project"}],
                "promote_spaces": [{"scope": "project"}],
                "subject": objective.trim(),
            }
        }
    });
    if validator_is_research_brief {
        if let Some(builder_object) = builder.get_mut("builder").and_then(Value::as_object_mut) {
            builder_object.insert(
                "web_research_expected".to_string(),
                Value::Bool(expects_web_research),
            );
        }
    }
    Some(builder)
}

pub fn workflow_step_enforcement_defaults(
    step_id: &str,
    kind: &str,
    objective: &str,
    validator_is_research_brief: bool,
) -> Option<Value> {
    if !validator_is_research_brief {
        return None;
    }
    let expects_web_research = workflow_step_expects_web_research(step_id, kind, objective);
    let normalized_kind = kind.trim().to_ascii_lowercase();
    serde_json::to_value(research_output_contract_policy_seed(
        &normalized_kind,
        expects_web_research,
        tandem_core::prewrite_repair_retry_max_attempts() as u32,
    ))
    .ok()
}

pub fn normalize_workflow_step_metadata<
    Step,
    StepId,
    Kind,
    Objective,
    IsBrief,
    IsEnforcementNone,
    SetEnforcement,
    Metadata,
    SetMetadata,
>(
    step: &mut Step,
    step_id: StepId,
    kind: Kind,
    objective: Objective,
    output_contract_is_research_brief: IsBrief,
    output_contract_enforcement_is_none: IsEnforcementNone,
    mut set_output_contract_enforcement_from_value: SetEnforcement,
    metadata: Metadata,
    mut set_metadata: SetMetadata,
) where
    StepId: Fn(&Step) -> &str,
    Kind: Fn(&Step) -> &str,
    Objective: Fn(&Step) -> &str,
    IsBrief: Fn(&Step) -> bool,
    IsEnforcementNone: Fn(&Step) -> bool,
    SetEnforcement: FnMut(&mut Step, Value),
    Metadata: Fn(&Step) -> Option<&Value>,
    SetMetadata: FnMut(&mut Step, Value),
{
    let validator_is_research_brief = output_contract_is_research_brief(step);
    if let Some(enforcement) = workflow_step_enforcement_defaults(
        step_id(step),
        kind(step),
        objective(step),
        validator_is_research_brief,
    ) {
        if output_contract_enforcement_is_none(step) {
            set_output_contract_enforcement_from_value(step, enforcement);
        }
    }
    let defaults = workflow_step_metadata_defaults(
        step_id(step),
        kind(step),
        objective(step),
        validator_is_research_brief,
    );
    match (metadata(step).cloned(), defaults) {
        (Some(mut metadata), Some(defaults)) => {
            let Some(root) = metadata.as_object_mut() else {
                set_metadata(step, defaults);
                return;
            };
            let builder = root
                .entry("builder".to_string())
                .or_insert_with(|| json!({}));
            let Some(builder_map) = builder.as_object_mut() else {
                *builder = defaults
                    .get("builder")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                set_metadata(step, metadata);
                return;
            };
            if let Some(default_builder) = defaults.get("builder").and_then(Value::as_object) {
                for (key, value) in default_builder {
                    builder_map
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
            }
            set_metadata(step, metadata);
        }
        (None, Some(defaults)) => {
            set_metadata(step, defaults);
        }
        _ => {}
    }
}

fn workflow_step_builder_map_mut(
    step: &mut WorkflowPlanStep<impl WorkflowInputRefLike, impl Serialize>,
) -> Option<&mut serde_json::Map<String, Value>> {
    let metadata = step.metadata.get_or_insert_with(|| json!({}));
    let root = metadata.as_object_mut()?;
    let builder = root
        .entry("builder".to_string())
        .or_insert_with(|| json!({}));
    builder.as_object_mut()
}

fn workflow_step_builder_string_array(
    step: &WorkflowPlanStep<impl WorkflowInputRefLike, impl Serialize>,
    key: &str,
) -> Vec<String> {
    step.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get(key))
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
        .unwrap_or_default()
}

fn workflow_step_builder_string(
    step: &WorkflowPlanStep<impl WorkflowInputRefLike, impl Serialize>,
    key: &str,
) -> Option<String> {
    step.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn workflow_step_decomposition_metadata_defaults(
    step: &mut WorkflowPlanStep<impl WorkflowInputRefLike, impl Serialize>,
    profile: &crate::decomposition::WorkflowDecompositionProfile,
    step_index: usize,
    step_count: usize,
) {
    let step_id = step.step_id.clone();
    let kind = step.kind.clone();
    let objective = step.objective.clone();
    let output_contract_kind = step
        .output_contract
        .as_ref()
        .and_then(|contract| serde_json::to_value(contract).ok())
        .and_then(|contract| contract.get("kind").cloned())
        .and_then(|value| value.as_str().map(str::to_string));
    let hints = derive_step_decomposition_hints(
        &step_id,
        &kind,
        &objective,
        output_contract_kind.as_deref(),
        &step.depends_on,
        step_index,
        step_count,
        profile,
    );
    let Some(builder) = workflow_step_builder_map_mut(step) else {
        return;
    };
    builder
        .entry("phase_id".to_string())
        .or_insert_with(|| Value::String(hints.phase_id.clone()));
    builder
        .entry("task_class".to_string())
        .or_insert_with(|| Value::String(hints.task_class.clone()));
    builder
        .entry("task_kind".to_string())
        .or_insert_with(|| Value::String(hints.task_class.clone()));
    builder
        .entry("task_family".to_string())
        .or_insert_with(|| Value::String(hints.task_family.clone()));
    builder
        .entry("retry_class".to_string())
        .or_insert_with(|| Value::String(hints.retry_class.clone()));
    if let Some(parent_step_id) = hints.parent_step_id {
        builder
            .entry("parent_step_id".to_string())
            .or_insert_with(|| Value::String(parent_step_id));
    }
}

pub fn derive_workflow_step_file_contracts<S, I, O>(
    plan: &mut WorkflowPlan<S, WorkflowPlanStep<I, O>>,
) where
    I: WorkflowInputRefLike + Serialize,
    O: Serialize,
{
    let output_files_by_step_id = plan
        .steps
        .iter()
        .map(|step| {
            let mut output_files = workflow_step_builder_string_array(step, "output_files");
            if output_files.is_empty() {
                if let Some(output_path) = workflow_step_builder_string(step, "output_path") {
                    output_files.push(output_path);
                }
            }
            output_files.sort();
            output_files.dedup();
            (step.step_id.clone(), output_files)
        })
        .collect::<std::collections::HashMap<_, _>>();

    for step in &mut plan.steps {
        let explicit_input_files = step
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(Value::as_object)
            .is_some_and(|builder| builder.contains_key("input_files"));
        let explicit_output_files = step
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(Value::as_object)
            .is_some_and(|builder| builder.contains_key("output_files"));
        let inferred_input_files = step
            .input_refs
            .iter()
            .flat_map(|input_ref| {
                output_files_by_step_id
                    .get(input_ref.from_step_id())
                    .cloned()
                    .unwrap_or_default()
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let inferred_output_files = output_files_by_step_id
            .get(&step.step_id)
            .cloned()
            .unwrap_or_default();
        let Some(builder) = workflow_step_builder_map_mut(step) else {
            continue;
        };
        if !explicit_input_files && !inferred_input_files.is_empty() {
            builder.insert("input_files".to_string(), json!(inferred_input_files));
        }
        if !explicit_output_files && !inferred_output_files.is_empty() {
            builder.insert("output_files".to_string(), json!(inferred_output_files));
        }
    }
}

pub fn inferred_output_validator_kind(contract_kind: &str) -> ProjectedOutputValidatorKind {
    match contract_kind.trim().to_ascii_lowercase().as_str() {
        "brief" => ProjectedOutputValidatorKind::ResearchBrief,
        "review" | "review_summary" | "approval_gate" => {
            ProjectedOutputValidatorKind::ReviewDecision
        }
        "structured_json" => ProjectedOutputValidatorKind::StructuredJson,
        "code_patch" => ProjectedOutputValidatorKind::CodePatch,
        _ => ProjectedOutputValidatorKind::GenericArtifact,
    }
}

pub fn output_contract_is_code_patch(
    contract_kind: &str,
    explicit_validator_key: Option<&str>,
) -> bool {
    explicit_validator_key
        .map(crate::contracts::projected_output_validator_kind_from_key)
        .unwrap_or_else(|| inferred_output_validator_kind(contract_kind))
        == ProjectedOutputValidatorKind::CodePatch
}

pub fn output_contract_is_research_brief(
    contract_kind: &str,
    explicit_validator_key: Option<&str>,
) -> bool {
    explicit_validator_key
        .map(crate::contracts::projected_output_validator_kind_from_key)
        .unwrap_or_else(|| inferred_output_validator_kind(contract_kind))
        == ProjectedOutputValidatorKind::ResearchBrief
}

pub fn plan_step_with_dep<InputRef, OutputContract>(
    step_id: &str,
    kind: &str,
    objective: &str,
    agent_role: &str,
    depends_on: &[String],
    input_refs: Vec<InputRef>,
    output_contract: Option<OutputContract>,
    metadata: Option<Value>,
) -> WorkflowPlanStep<InputRef, OutputContract> {
    WorkflowPlanStep {
        step_id: step_id.to_string(),
        kind: kind.to_string(),
        objective: objective.to_string(),
        depends_on: depends_on.to_vec(),
        agent_role: agent_role.to_string(),
        input_refs,
        output_contract,
        metadata,
    }
}

pub fn build_minimal_fallback_plan<S, Step>(
    plan_id: &str,
    planner_version: &str,
    plan_source: &str,
    prompt: &str,
    normalized_prompt: &str,
    title: String,
    workspace_root: String,
    schedule: S,
    allowed_mcp_servers: Vec<String>,
    operator_preferences: Option<Value>,
    description: Option<String>,
    fallback_step: Step,
) -> WorkflowPlan<S, Step> {
    WorkflowPlan {
        plan_id: plan_id.to_string(),
        planner_version: planner_version.to_string(),
        plan_source: plan_source.to_string(),
        original_prompt: prompt.trim().to_string(),
        normalized_prompt: normalized_prompt.to_string(),
        confidence: "low".to_string(),
        title,
        description,
        schedule,
        execution_target: "automation_v2".to_string(),
        workspace_root,
        steps: vec![fallback_step],
        requires_integrations: Vec::new(),
        allowed_mcp_servers,
        operator_preferences,
        save_options: plan_save_options(),
    }
}

pub fn plan_title(prompt: &str, schedule_type: &AutomationV2ScheduleType) -> String {
    let trimmed = prompt
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if trimmed.is_empty() {
        return "Automation Plan".to_string();
    }
    let base = if trimmed.len() > 64 {
        format!("{}...", &trimmed[..61])
    } else {
        trimmed
    };
    match schedule_type {
        AutomationV2ScheduleType::Manual => base,
        _ => format!("Scheduled {}", base),
    }
}

pub fn normalize_prompt(prompt: &str) -> String {
    prompt
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

pub fn normalize_string_list(raw: Vec<String>) -> Vec<String> {
    let mut values = raw
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub fn infer_explicit_output_targets(prompt: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for raw_token in prompt.split_whitespace() {
        let token = raw_token
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':'
                )
            })
            .trim();
        if token.is_empty() || token.contains("://") {
            continue;
        }
        let path = Path::new(token);
        let has_extension = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| !value.is_empty());
        let looks_like_path = token.starts_with('/')
            || token.starts_with("./")
            || token.starts_with("../")
            || token.contains('/');
        if looks_like_path && has_extension {
            targets.push(token.to_string());
        }
    }
    targets.sort();
    targets.dedup();
    targets
}

pub fn workflow_plan_mentions_connector_backed_sources(prompt: &str) -> bool {
    let lowered = prompt.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return false;
    }
    [
        "mcp",
        "reddit",
        "github issue",
        "github issues",
        "slack",
        "jira",
        "linear",
        "notion",
        "confluence",
        "zendesk",
        "salesforce",
        "airtable",
        "google drive",
        "google docs",
        "google sheets",
        "gmail",
        "outlook",
        "sharepoint",
        "dropbox",
        "discord",
        "intercom",
        "figma",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

pub fn workflow_plan_should_surface_mcp_discovery(
    prompt: &str,
    allowed_mcp_servers: &[String],
) -> bool {
    !allowed_mcp_servers.is_empty() || workflow_plan_mentions_connector_backed_sources(prompt)
}

pub fn plan_max_parallel_agents(operator_preferences: Option<&Value>) -> u32 {
    operator_preferences
        .and_then(|prefs| prefs.get("max_parallel_agents"))
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 16) as u32)
        .or_else(|| {
            operator_preferences
                .and_then(|prefs| prefs.get("execution_mode"))
                .and_then(Value::as_str)
                .map(str::trim)
                .and_then(|mode| match mode {
                    "swarm" => Some(4),
                    _ => Some(1),
                })
        })
        .unwrap_or(1)
}

pub fn workflow_plan_agent_roles<Step, F>(steps: &[Step], agent_role: F) -> Vec<String>
where
    F: Fn(&Step) -> &str,
{
    let mut roles = BTreeSet::new();
    for step in steps {
        let role = agent_role(step).trim();
        if !role.is_empty() {
            roles.insert(role.to_string());
        }
    }
    roles.into_iter().collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerMcpServerToolSet {
    pub server: String,
    #[serde(default)]
    pub tool_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerMcpServerCapabilitySummary {
    pub server: String,
    pub tool_count: usize,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub sample_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerCapabilitySummary {
    #[serde(default)]
    pub built_in_capabilities: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<PlannerMcpServerCapabilitySummary>,
}

pub fn build_planner_capability_summary(server_tools: &[PlannerMcpServerToolSet]) -> Value {
    let mut servers = Vec::new();
    for server in server_tools {
        let mut capabilities = BTreeSet::new();
        let mut sample_tools = Vec::new();
        for tool_name in server.tool_names.iter().take(8) {
            let tool_name = tool_name.trim().to_string();
            if !tool_name.is_empty() {
                sample_tools.push(tool_name.clone());
            }
            let lower = tool_name.to_ascii_lowercase();
            if lower.contains("gmail_send_email") || lower.contains("send_email") {
                capabilities.insert("email_send".to_string());
            }
            if lower.contains("gmail_send_draft") || lower.contains("send_draft") {
                capabilities.insert("email_draft".to_string());
            }
            if lower.contains("reddit") {
                capabilities.insert("reddit_research".to_string());
            }
            if lower.contains("search") {
                capabilities.insert("search".to_string());
            }
            if lower.contains("docs") || lower.contains("document") {
                capabilities.insert("docs".to_string());
            }
            if lower.contains("slack") {
                capabilities.insert("slack_delivery".to_string());
            }
        }
        servers.push(PlannerMcpServerCapabilitySummary {
            server: server.server.clone(),
            tool_count: server.tool_names.len(),
            capabilities: capabilities.into_iter().collect(),
            sample_tools,
        });
    }
    json!(PlannerCapabilitySummary {
        built_in_capabilities: vec![
            "web_research".to_string(),
            "web_fetch".to_string(),
            "workspace_read".to_string(),
        ],
        mcp_servers: servers,
    })
}

pub fn normalize_operator_preferences(raw: Option<Value>) -> Option<Value> {
    let Some(mut prefs) = raw else {
        return None;
    };
    let Some(map) = prefs.as_object_mut() else {
        return None;
    };
    if let Some(mode) = map
        .get("execution_mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert(
            "execution_mode".to_string(),
            Value::String(mode.to_string()),
        );
    } else {
        map.remove("execution_mode");
    }
    if let Some(max_parallel) = map.get("max_parallel_agents").and_then(Value::as_u64) {
        map.insert(
            "max_parallel_agents".to_string(),
            Value::Number(serde_json::Number::from(max_parallel.clamp(1, 16))),
        );
    } else {
        map.remove("max_parallel_agents");
    }
    if let Some(provider_id) = map
        .get("model_provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert(
            "model_provider".to_string(),
            Value::String(provider_id.to_string()),
        );
    } else {
        map.remove("model_provider");
    }
    if let Some(model_id) = map
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert("model_id".to_string(), Value::String(model_id.to_string()));
    } else {
        map.remove("model_id");
    }
    if let Some(role_models) = map.get_mut("role_models").and_then(Value::as_object_mut) {
        let invalid_role_keys = role_models
            .iter()
            .filter_map(|(key, value)| {
                let role = value.as_object()?;
                let provider_id = role
                    .get("provider_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let model_id = role
                    .get("model_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if provider_id.is_some() && model_id.is_some() {
                    None
                } else {
                    Some(key.clone())
                }
            })
            .collect::<Vec<_>>();
        for key in invalid_role_keys {
            role_models.remove(&key);
        }
        if role_models.is_empty() {
            map.remove("role_models");
        }
    } else {
        map.remove("role_models");
    }
    if map.is_empty() {
        None
    } else {
        Some(prefs)
    }
}

pub fn merge_create_operator_preferences(
    explicit: Option<&Value>,
    candidate: Option<Value>,
) -> Option<Value> {
    let candidate = normalize_operator_preferences(candidate);
    let explicit = normalize_operator_preferences(explicit.cloned());
    match (candidate, explicit) {
        (None, None) => None,
        (Some(candidate), None) => Some(candidate),
        (None, Some(explicit)) => Some(explicit),
        (Some(candidate), Some(explicit)) => {
            let mut merged = candidate.as_object().cloned().unwrap_or_default();
            for (key, value) in explicit.as_object().cloned().unwrap_or_default() {
                merged.insert(key, value);
            }
            normalize_operator_preferences(Some(Value::Object(merged)))
        }
    }
}

pub fn extract_json_value_from_text(text: &str) -> Option<Value> {
    serde_json::from_str(text.trim())
        .ok()
        .or_else(|| {
            text.split("```").find_map(|chunk| {
                let trimmed = chunk.trim();
                if trimmed.starts_with('{') || trimmed.starts_with('[') {
                    serde_json::from_str(trimmed).ok()
                } else if let Some(rest) = trimmed.strip_prefix("json") {
                    let rest = rest.trim();
                    (rest.starts_with('{') || rest.starts_with('['))
                        .then(|| serde_json::from_str(rest).ok())
                        .flatten()
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            extract_balanced_json_fragment(text)
                .and_then(|fragment| serde_json::from_str::<Value>(fragment).ok())
        })
}

fn extract_balanced_json_fragment(text: &str) -> Option<&str> {
    let mut start = None;
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escape = false;

    for (idx, ch) in text.char_indices() {
        if start.is_none() {
            if ch == '{' || ch == '[' {
                start = Some(idx);
                stack.push(ch);
            }
            continue;
        }

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
                let Some(open) = stack.pop() else {
                    return None;
                };
                let is_match = matches!((open, ch), ('{', '}') | ('[', ']'));
                if !is_match {
                    return None;
                }
                if stack.is_empty() {
                    let start = start?;
                    return Some(&text[start..idx + ch.len_utf8()]);
                }
            }
            _ => {}
        }
    }

    None
}

pub fn decode_planner_plan_value<Plan: DeserializeOwned>(value: Value) -> Option<Plan> {
    serde_json::from_value::<Plan>(value.clone())
        .ok()
        .or_else(|| decode_planner_plan_value_relaxed(value))
}

pub fn decode_planner_plan_value_relaxed<Plan: DeserializeOwned>(mut value: Value) -> Option<Plan> {
    let plan = value.as_object_mut()?;
    plan.entry("plan_id".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("planner_version".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("plan_source".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("original_prompt".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("normalized_prompt".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("confidence".to_string())
        .or_insert_with(|| Value::String("medium".to_string()));
    plan.entry("title".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("save_options".to_string())
        .or_insert_with(|| json!({}));
    plan.entry("requires_integrations".to_string())
        .or_insert_with(|| json!([]));
    plan.entry("allowed_mcp_servers".to_string())
        .or_insert_with(|| json!([]));
    let steps = plan.get_mut("steps")?.as_array_mut()?;
    for step in steps.iter_mut() {
        let Some(step_obj) = step.as_object_mut() else {
            continue;
        };
        if !step_obj.contains_key("step_id") {
            if let Some(id) = step_obj.get("id").cloned() {
                step_obj.insert("step_id".to_string(), id);
            }
        }
        if !step_obj.contains_key("kind") {
            if let Some(kind) = step_obj.get("type").cloned() {
                step_obj.insert("kind".to_string(), kind);
            }
        }
        if !step_obj.contains_key("objective") {
            let objective = step_obj
                .get("objective")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    step_obj
                        .get("config")
                        .and_then(|row| row.get("objective"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .or_else(|| {
                    step_obj
                        .get("label")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            if let Some(objective) = objective {
                step_obj.insert("objective".to_string(), Value::String(objective));
            }
        }
        if !step_obj.contains_key("agent_role") {
            let agent_role = step_obj
                .get("agent_role")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    step_obj
                        .get("config")
                        .and_then(|row| row.get("agent_role"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "worker".to_string());
            step_obj.insert("agent_role".to_string(), Value::String(agent_role));
        }
        if let Some(input_refs) = step_obj.get_mut("input_refs").and_then(Value::as_array_mut) {
            for input_ref in input_refs.iter_mut() {
                match input_ref {
                    Value::String(from_step_id) => {
                        *input_ref = json!({
                            "from_step_id": from_step_id,
                            "alias": from_step_id
                        });
                    }
                    Value::Object(map) => {
                        if !map.contains_key("from_step_id") {
                            if let Some(value) = map
                                .get("from")
                                .cloned()
                                .or_else(|| map.get("step_id").cloned())
                                .or_else(|| map.get("id").cloned())
                            {
                                map.insert("from_step_id".to_string(), value);
                            }
                        }
                        if !map.contains_key("alias") {
                            if let Some(from_step_id) = map
                                .get("from_step_id")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                            {
                                map.insert("alias".to_string(), Value::String(from_step_id));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if !step_obj.contains_key("output_contract") {
            let inferred_kind = step_obj
                .get("config")
                .and_then(|row| row.get("format"))
                .and_then(Value::as_str)
                .and_then(|format| match format.trim().to_ascii_lowercase().as_str() {
                    "markdown" | "md" => Some("report_markdown".to_string()),
                    "json" => Some("structured_json".to_string()),
                    "text" | "summary" => Some("text_summary".to_string()),
                    _ => None,
                });
            if let Some(kind) = inferred_kind {
                step_obj.insert(
                    "output_contract".to_string(),
                    json!({
                        "kind": kind
                    }),
                );
            }
        }
    }
    serde_json::from_value::<Plan>(value).ok()
}

pub fn manual_schedule<M: Clone>(timezone: String, misfire_policy: M) -> AutomationV2Schedule<M> {
    AutomationV2Schedule {
        schedule_type: AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone,
        misfire_policy,
    }
}

pub fn schedule_from_value<M: Clone>(
    value: &Value,
    default_misfire_policy: M,
) -> Option<AutomationV2Schedule<M>> {
    let timezone = value
        .get("timezone")
        .and_then(Value::as_str)
        .unwrap_or("UTC")
        .to_string();
    if let Some(schedule_type) = value.get("type").and_then(Value::as_str) {
        match schedule_type.trim().to_ascii_lowercase().as_str() {
            "cron" => {
                let expr = value
                    .get("cron_expression")
                    .or_else(|| value.get("cronExpression"))
                    .and_then(Value::as_str)?
                    .trim()
                    .to_string();
                if expr.is_empty() {
                    return None;
                }
                return Some(AutomationV2Schedule {
                    schedule_type: AutomationV2ScheduleType::Cron,
                    cron_expression: Some(expr),
                    interval_seconds: None,
                    timezone,
                    misfire_policy: default_misfire_policy,
                });
            }
            "interval" => {
                let seconds = value
                    .get("interval_seconds")
                    .or_else(|| value.get("intervalSeconds"))
                    .and_then(Value::as_u64)?;
                return Some(AutomationV2Schedule {
                    schedule_type: AutomationV2ScheduleType::Interval,
                    cron_expression: None,
                    interval_seconds: Some(seconds),
                    timezone,
                    misfire_policy: default_misfire_policy,
                });
            }
            "manual" => {
                return Some(AutomationV2Schedule {
                    schedule_type: AutomationV2ScheduleType::Manual,
                    cron_expression: None,
                    interval_seconds: None,
                    timezone,
                    misfire_policy: default_misfire_policy,
                });
            }
            _ => {}
        }
    }
    if let Some(expr) = value
        .get("cron")
        .and_then(|cron| {
            cron.get("expression")
                .or_else(|| cron.get("cron_expression"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Cron,
            cron_expression: Some(expr.to_string()),
            interval_seconds: None,
            timezone,
            misfire_policy: default_misfire_policy,
        });
    }
    let seconds = value.get("interval_seconds").and_then(|row| {
        row.get("seconds")
            .and_then(Value::as_u64)
            .or_else(|| row.as_u64())
    });
    seconds.map(|seconds| AutomationV2Schedule {
        schedule_type: AutomationV2ScheduleType::Interval,
        cron_expression: None,
        interval_seconds: Some(seconds),
        timezone,
        misfire_policy: default_misfire_policy,
    })
}

pub fn workflow_steps_equal<I: Serialize, O: Serialize>(
    left: &[WorkflowPlanStep<I, O>],
    right: &[WorkflowPlanStep<I, O>],
) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

pub fn workflow_schedule_equal<M: Serialize>(
    left: &AutomationV2Schedule<M>,
    right: &AutomationV2Schedule<M>,
) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

pub trait WorkflowInputRefLike {
    fn from_step_id(&self) -> &str;
}

pub fn validate_workflow_plan<S, I, O>(
    plan: &WorkflowPlan<S, WorkflowPlanStep<I, O>>,
) -> Result<(), String>
where
    I: WorkflowInputRefLike,
{
    if plan.execution_target.trim() != "automation_v2" {
        return Err("execution_target must be automation_v2".to_string());
    }
    normalize_absolute_workspace_root(&plan.workspace_root)?;
    let allowed_step_ids = allowed_workflow_step_ids();
    let step_ids = plan
        .steps
        .iter()
        .map(|step| normalize_workflow_step_id(step.step_id.as_str()))
        .collect::<std::collections::HashSet<_>>();
    if step_ids.is_empty() {
        return Err("workflow plan must include at least one step".to_string());
    }
    for step in &plan.steps {
        let normalized_step_id = normalize_workflow_step_id(step.step_id.as_str());
        if !workflow_step_id_is_allowed(&normalized_step_id, &allowed_step_ids) {
            return Err(format!("unsupported workflow step id `{}`", step.step_id));
        }
        for dep in &step.depends_on {
            if !step_ids.contains(&normalize_workflow_step_id(dep.as_str())) {
                return Err(format!(
                    "workflow step `{}` depends on unknown step `{}`",
                    step.step_id, dep
                ));
            }
        }
        for input in &step.input_refs {
            if !step_ids.contains(&normalize_workflow_step_id(input.from_step_id())) {
                return Err(format!(
                    "workflow step `{}` references unknown input step `{}`",
                    step.step_id,
                    input.from_step_id()
                ));
            }
        }
    }
    Ok(())
}

pub const ALLOWED_WORKFLOW_STEP_IDS: &[&str] = &[
    "assess",
    "collect_inputs",
    "research_sources",
    "extract_pain_points",
    "cluster_topics",
    "analyze_findings",
    "generate_report",
    "compare_results",
    "compare_with_features",
    "notify_user",
    "execute_goal",
];

fn allowed_workflow_step_ids() -> std::collections::HashSet<&'static str> {
    ALLOWED_WORKFLOW_STEP_IDS.iter().copied().collect()
}

/// Returns `true` when a step ID or kind indicates a triage / awareness check.
/// The compiler uses this to apply triage-gate metadata automatically.
pub fn workflow_step_is_triage(step_id: &str, kind: &str) -> bool {
    let lowered_id = step_id.trim().to_ascii_lowercase();
    let lowered_kind = kind.trim().to_ascii_lowercase();
    lowered_id.contains("assess")
        || lowered_id.contains("triage")
        || lowered_kind.contains("assess")
        || lowered_kind.contains("triage")
}

fn normalize_workflow_step_id(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn workflow_step_id_is_allowed(
    normalized_step_id: &str,
    allowed_step_ids: &std::collections::HashSet<&'static str>,
) -> bool {
    if allowed_step_ids.contains(normalized_step_id) {
        return true;
    }
    allowed_step_ids.iter().any(|allowed| {
        normalized_step_id
            .strip_prefix(*allowed)
            .is_some_and(|suffix| suffix.starts_with('_') && suffix.len() > 1)
    })
}

pub fn normalize_absolute_workspace_root(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("workspace_root is required".to_string());
    }
    let as_path = PathBuf::from(trimmed);
    if !as_path.is_absolute() {
        return Err("workspace_root must be an absolute path".to_string());
    }
    tandem_core::normalize_workspace_path(trimmed)
        .ok_or_else(|| "workspace_root is invalid".to_string())
}

pub fn resolve_workspace_root_candidate(
    requested: Option<&str>,
    default_root: &str,
    cwd: Option<&str>,
) -> Result<String, String> {
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());
    if let Some(workspace_root) = requested {
        return normalize_absolute_workspace_root(workspace_root);
    }

    match normalize_absolute_workspace_root(default_root) {
        Ok(normalized) => Ok(normalized),
        Err(error) => {
            #[cfg(unix)]
            {
                if default_root.starts_with('\\') {
                    let unix_like = default_root.replace('\\', "/");
                    return normalize_absolute_workspace_root(&unix_like);
                }
            }

            let cwd = cwd.ok_or(error.clone())?;
            normalize_absolute_workspace_root(cwd)
        }
    }
}

pub fn normalize_and_validate_planner_plan<M, I, O>(
    mut candidate: WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    ctx: &PlannerPlanNormalizationContext<'_, M>,
    mut normalize_step: impl FnMut(&mut WorkflowPlanStep<I, O>),
) -> Result<WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>, String>
where
    M: Clone,
    I: WorkflowInputRefLike,
    O: Serialize,
{
    candidate.plan_id = ctx.plan_id.to_string();
    candidate.planner_version = ctx.planner_version.to_string();
    candidate.plan_source = ctx.plan_source.to_string();
    candidate.original_prompt = ctx.original_prompt.to_string();
    candidate.normalized_prompt = ctx.normalized_prompt.to_string();
    candidate.execution_target = "automation_v2".to_string();
    candidate.requires_integrations = normalize_string_list(candidate.requires_integrations);
    candidate.description = candidate.description.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed.to_string())
    });
    candidate.confidence = match candidate.confidence.trim().to_ascii_lowercase().as_str() {
        "low" | "medium" | "high" => candidate.confidence.trim().to_ascii_lowercase(),
        _ => "medium".to_string(),
    };
    candidate.title = {
        let trimmed = candidate.title.trim();
        if trimmed.is_empty() {
            plan_title(ctx.original_prompt, &candidate.schedule.schedule_type)
        } else {
            truncate_text(trimmed, 120)
        }
    };
    candidate.save_options = if candidate.save_options.is_object() {
        candidate.save_options
    } else {
        plan_save_options()
    };

    match ctx.mode {
        PlannerPlanMode::Create => {
            candidate.workspace_root = ctx.resolved_workspace_root.to_string();
            candidate.allowed_mcp_servers = ctx.request_allowed_mcp_servers.to_vec();
            candidate.operator_preferences = merge_create_operator_preferences(
                ctx.request_operator_preferences,
                candidate.operator_preferences.take(),
            );
            if let Some(explicit_schedule) = ctx.explicit_schedule {
                candidate.schedule = explicit_schedule.clone();
            }
        }
        PlannerPlanMode::Revise => {
            candidate.workspace_root =
                normalize_absolute_workspace_root(&candidate.workspace_root)?;
            candidate.allowed_mcp_servers = normalize_string_list(candidate.allowed_mcp_servers);
            candidate.operator_preferences =
                normalize_operator_preferences(candidate.operator_preferences.take());
        }
    }

    let decomposition_profile = derive_workflow_decomposition_profile(
        ctx.original_prompt,
        &candidate.allowed_mcp_servers,
        &infer_explicit_output_targets(ctx.original_prompt),
        !matches!(
            &candidate.schedule.schedule_type,
            AutomationV2ScheduleType::Manual
        ),
    );
    let step_count = candidate.steps.len();
    for step in &mut candidate.steps {
        normalize_step(step);
    }
    for (step_index, step) in candidate.steps.iter_mut().enumerate() {
        workflow_step_decomposition_metadata_defaults(
            step,
            &decomposition_profile,
            step_index,
            step_count,
        );
    }

    validate_workflow_plan(&candidate)?;
    Ok(candidate)
}

pub enum PlannerPlanMode {
    Create,
    Revise,
}

pub struct PlannerPlanNormalizationContext<'a, M> {
    pub mode: PlannerPlanMode,
    pub plan_id: &'a str,
    pub planner_version: &'a str,
    pub plan_source: &'a str,
    pub original_prompt: &'a str,
    pub normalized_prompt: &'a str,
    pub resolved_workspace_root: &'a str,
    pub explicit_schedule: Option<&'a AutomationV2Schedule<M>>,
    pub request_allowed_mcp_servers: &'a [String],
    pub request_operator_preferences: Option<&'a Value>,
}

pub fn normalize_mcp_server_namespace(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "server".to_string()
    } else {
        cleaned.to_string()
    }
}

pub fn compile_workflow_agent_tool_allowlist(
    allowed_mcp_servers: &[String],
    operator_preferences: Option<&Value>,
    normalize_allowed_tools: impl Fn(Vec<String>) -> Vec<String>,
) -> Vec<String> {
    let custom_allowlist = operator_preferences
        .and_then(|prefs| prefs.get("tool_allowlist"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tool_access_mode = operator_preferences
        .and_then(|prefs| prefs.get("tool_access_mode"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("all");
    let mut allowlist = if tool_access_mode == "custom" {
        custom_allowlist
    } else {
        vec!["*".to_string()]
    };
    for server in allowed_mcp_servers {
        let namespace = normalize_mcp_server_namespace(server);
        allowlist.push(format!("mcp.{namespace}.*"));
    }
    normalize_allowed_tools(allowlist)
}

pub fn planner_model_spec(operator_preferences: Option<&Value>) -> Option<tandem_types::ModelSpec> {
    let prefs = operator_preferences?;
    let default_provider = prefs
        .get("model_provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let default_model = prefs
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(planner_model) = prefs
        .get("role_models")
        .and_then(|row| row.get("planner"))
        .and_then(Value::as_object)
    {
        let provider_id = planner_model
            .get("provider_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let model_id = planner_model
            .get("model_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let (Some(provider_id), Some(model_id)) = (provider_id, model_id) {
            return Some(tandem_types::ModelSpec {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
            });
        }
    }
    match (default_provider, default_model) {
        (Some(provider_id), Some(model_id)) => Some(tandem_types::ModelSpec {
            provider_id,
            model_id,
        }),
        _ => None,
    }
}

pub fn compile_operator_model_policy(operator_preferences: Option<&Value>) -> Option<Value> {
    let prefs = operator_preferences?;
    let provider_id = prefs
        .get("model_provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model_id = prefs
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let role_models = prefs
        .get("role_models")
        .cloned()
        .filter(|value| value.is_object());
    let mut payload = serde_json::Map::new();
    if let (Some(provider_id), Some(model_id)) = (provider_id, model_id) {
        payload.insert(
            "default_model".to_string(),
            json!({
                "provider_id": provider_id,
                "model_id": model_id,
            }),
        );
    }
    if let Some(role_models) = role_models {
        payload.insert("role_models".to_string(), role_models);
    }
    if payload.is_empty() {
        None
    } else {
        Some(Value::Object(payload))
    }
}

pub fn agent_id_for_role(role: &str) -> String {
    format!("agent_{}", role.trim().replace([' ', '-'], "_"))
}

pub fn display_name_for_role(role: &str) -> String {
    role.split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn planner_llm_provider_unconfigured_hint(provider_id: &str) -> String {
    format!(
        "The configured planner model uses provider `{provider_id}`, but that provider is not configured on this engine. Configure the provider first and try again."
    )
}

pub(crate) fn planner_diagnostics(
    reason: Option<&str>,
    detail: Option<String>,
    decomposition_observation: Option<Value>,
) -> Option<Value> {
    let mut payload = serde_json::Map::new();
    if let Some(reason) = reason.map(str::trim).filter(|value| !value.is_empty()) {
        payload.insert(
            "fallback_reason".to_string(),
            Value::String(reason.to_string()),
        );
    }
    if let Some(detail) = detail
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        payload.insert("detail".to_string(), Value::String(detail));
    }
    if let Some(decomposition_observation) = decomposition_observation {
        if let Some(observation) = decomposition_observation.as_object() {
            for (key, value) in observation {
                payload.insert(key.clone(), value.clone());
            }
        } else {
            payload.insert(
                "decomposition_profile".to_string(),
                decomposition_observation,
            );
        }
    }
    if payload.is_empty() {
        None
    } else {
        Some(Value::Object(payload))
    }
}

pub(crate) fn truncate_text(input: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut out = input[..max_len].to_string();
    out.truncate(max_len);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decomposition::workflow_plan_decomposition_observation;
    use tandem_workflows::plan_package::WorkflowPlanStep;

    fn test_plan_with_steps(
        steps: Vec<WorkflowPlanStep<Value, Value>>,
    ) -> WorkflowPlan<AutomationV2Schedule<Value>, WorkflowPlanStep<Value, Value>> {
        WorkflowPlan {
            plan_id: "wfplan-test".to_string(),
            planner_version: "v1".to_string(),
            plan_source: "unit_test".to_string(),
            original_prompt: "Test prompt".to_string(),
            normalized_prompt: "test prompt".to_string(),
            confidence: "medium".to_string(),
            title: "Test Plan".to_string(),
            description: None,
            schedule: manual_schedule("UTC".to_string(), json!({"type":"run_once"})),
            execution_target: "automation_v2".to_string(),
            workspace_root: "/tmp/workspace".to_string(),
            steps,
            requires_integrations: Vec::new(),
            allowed_mcp_servers: Vec::new(),
            operator_preferences: None,
            save_options: plan_save_options(),
        }
    }

    #[test]
    fn resolve_workspace_root_candidate_prefers_requested_root() {
        let resolved = resolve_workspace_root_candidate(
            Some("/tmp/requested"),
            "/tmp/default",
            Some("/tmp/cwd"),
        )
        .expect("requested root");

        assert_eq!(resolved, "/tmp/requested");
    }

    #[test]
    fn resolve_workspace_root_candidate_falls_back_to_cwd_when_default_is_invalid() {
        let resolved = resolve_workspace_root_candidate(None, "not-absolute", Some("/tmp/cwd"))
            .expect("cwd fallback");

        assert_eq!(resolved, "/tmp/cwd");
    }

    #[test]
    fn output_contract_is_research_brief_uses_explicit_or_inferred_validator() {
        assert!(output_contract_is_research_brief("brief", None));
        assert!(!output_contract_is_research_brief("structured_json", None));
        assert!(output_contract_is_research_brief(
            "structured_json",
            Some("research_brief")
        ));
        assert!(!output_contract_is_research_brief(
            "brief",
            Some("structured_json")
        ));
    }

    #[test]
    fn output_contract_is_code_patch_uses_explicit_or_inferred_validator() {
        assert!(output_contract_is_code_patch("code_patch", None));
        assert!(!output_contract_is_code_patch("structured_json", None));
        assert!(output_contract_is_code_patch(
            "structured_json",
            Some("code_patch")
        ));
        assert!(!output_contract_is_code_patch(
            "brief",
            Some("structured_json")
        ));
    }

    #[test]
    fn extract_json_value_from_text_handles_wrapped_json() {
        let text = r#"
Here is the planner response:

```json
{"action":"build","assistant_text":"ok","plan":{"title":"Demo","steps":[]}}
```
        "#;
        let value = extract_json_value_from_text(text).expect("json value");
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
    fn extract_json_value_from_text_handles_prefixed_json() {
        let text = r#"Planner output:
{"action":"clarify","assistant_text":"Need one detail","clarifier":{"field":"general","question":"Which repo?"}}
"#;
        let value = extract_json_value_from_text(text).expect("json value");
        assert_eq!(value.get("action").and_then(Value::as_str), Some("clarify"));
        assert_eq!(
            value
                .get("clarifier")
                .and_then(|clarifier| clarifier.get("question"))
                .and_then(Value::as_str),
            Some("Which repo?")
        );
    }

    #[test]
    fn planner_model_spec_falls_back_to_default_model() {
        let spec = planner_model_spec(Some(&json!({
            "model_provider": "openai",
            "model_id": "gpt-5.1"
        })))
        .expect("default planner spec");
        assert_eq!(spec.provider_id, "openai");
        assert_eq!(spec.model_id, "gpt-5.1");
    }

    #[test]
    fn workflow_step_metadata_defaults_include_project_knowledge() {
        let defaults =
            workflow_step_metadata_defaults("research_sources", "research", "Map the topic", false)
                .expect("metadata defaults");
        let builder = defaults
            .get("builder")
            .and_then(Value::as_object)
            .expect("builder");
        let knowledge = builder
            .get("knowledge")
            .and_then(Value::as_object)
            .expect("knowledge defaults");

        assert_eq!(
            knowledge.get("enabled").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            knowledge.get("reuse_mode").and_then(Value::as_str),
            Some("preflight")
        );
        assert_eq!(
            knowledge.get("trust_floor").and_then(Value::as_str),
            Some("promoted")
        );
        assert_eq!(
            knowledge.get("subject").and_then(Value::as_str),
            Some("Map the topic")
        );
        assert_eq!(
            knowledge
                .get("read_spaces")
                .and_then(Value::as_array)
                .and_then(|spaces| spaces.first())
                .and_then(|space| space.get("scope"))
                .and_then(Value::as_str),
            Some("project")
        );
        assert_eq!(
            knowledge
                .get("promote_spaces")
                .and_then(Value::as_array)
                .and_then(|spaces| spaces.first())
                .and_then(|space| space.get("scope"))
                .and_then(Value::as_str),
            Some("project")
        );
    }

    #[test]
    fn workflow_step_decomposition_metadata_defaults_add_phase_and_retry_hints() {
        let profile = crate::decomposition::WorkflowDecompositionProfile {
            complexity_score: 80,
            tier: crate::decomposition::WorkflowDecompositionTier::VeryComplex,
            recommended_min_leaf_tasks: 30,
            recommended_max_leaf_tasks: 50,
            recommended_phase_count: 4,
            requires_phased_dag: true,
            signals: vec!["scheduled_workflow".to_string()],
            guidance: vec!["Use phased microtasks.".to_string()],
        };
        let mut step: WorkflowPlanStep<Value, Value> = WorkflowPlanStep {
            step_id: "send_report".to_string(),
            kind: "deliver".to_string(),
            objective: "Send the report by email.".to_string(),
            depends_on: vec!["analyze_findings".to_string()],
            agent_role: "sender".to_string(),
            input_refs: vec![],
            output_contract: Some(json!({"kind":"report_markdown"})),
            metadata: None,
        };

        workflow_step_decomposition_metadata_defaults(&mut step, &profile, 3, 4);

        let builder = step
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("builder");
        assert_eq!(
            builder.get("phase_id").and_then(Value::as_str),
            Some("phase_4_deliver")
        );
        assert_eq!(
            builder.get("task_class").and_then(Value::as_str),
            Some("delivery")
        );
        assert_eq!(
            builder.get("task_kind").and_then(Value::as_str),
            Some("delivery")
        );
        assert_eq!(
            builder.get("retry_class").and_then(Value::as_str),
            Some("delivery_only")
        );
        assert_eq!(
            builder.get("parent_step_id").and_then(Value::as_str),
            Some("analyze_findings")
        );
    }

    #[test]
    fn planner_diagnostics_merges_decomposition_profile_into_payload() {
        let profile = crate::decomposition::WorkflowDecompositionProfile {
            complexity_score: 46,
            tier: crate::decomposition::WorkflowDecompositionTier::Complex,
            recommended_min_leaf_tasks: 20,
            recommended_max_leaf_tasks: 30,
            recommended_phase_count: 3,
            requires_phased_dag: true,
            signals: vec!["connector_backed_sources".to_string()],
            guidance: vec!["Use explicit phases.".to_string()],
        };
        let observation = workflow_plan_decomposition_observation(&profile, 12);
        let diagnostics = planner_diagnostics(None, None, Some(observation)).expect("diagnostics");

        assert_eq!(
            diagnostics
                .get("generated_step_count")
                .and_then(Value::as_u64),
            Some(12)
        );
        assert_eq!(
            diagnostics
                .get("decomposition_profile")
                .and_then(|value| value.get("recommended_phase_count"))
                .and_then(Value::as_u64),
            Some(3)
        );
    }

    #[test]
    fn derive_workflow_step_file_contracts_adds_upstream_input_and_output_files() {
        let mut plan = test_plan_with_steps(vec![
            WorkflowPlanStep {
                step_id: "collect_inputs".to_string(),
                kind: "collect".to_string(),
                objective: "Collect inputs.".to_string(),
                depends_on: vec![],
                agent_role: "planner".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: Some(json!({
                    "builder": {
                        "output_path": ".tandem/artifacts/collect-inputs.json"
                    }
                })),
            },
            WorkflowPlanStep {
                step_id: "draft_report".to_string(),
                kind: "write".to_string(),
                objective: "Draft the report.".to_string(),
                depends_on: vec!["collect_inputs".to_string()],
                agent_role: "writer".to_string(),
                input_refs: vec![json!({
                    "from_step_id": "collect_inputs",
                    "alias": "inputs"
                })],
                output_contract: Some(json!({"kind":"report_markdown"})),
                metadata: Some(json!({
                    "builder": {
                        "output_path": "reports/final.md"
                    }
                })),
            },
        ]);

        derive_workflow_step_file_contracts(&mut plan);

        let collect_builder = plan.steps[0]
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("collect builder");
        assert_eq!(
            collect_builder
                .get("output_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec![".tandem/artifacts/collect-inputs.json"])
        );
        let draft_builder = plan.steps[1]
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("draft builder");
        assert_eq!(
            draft_builder
                .get("input_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec![".tandem/artifacts/collect-inputs.json"])
        );
        assert_eq!(
            draft_builder
                .get("output_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec!["reports/final.md"])
        );
    }

    #[test]
    fn derive_workflow_step_file_contracts_preserves_explicit_contract_overrides() {
        let mut plan = test_plan_with_steps(vec![
            WorkflowPlanStep {
                step_id: "collect_inputs".to_string(),
                kind: "collect".to_string(),
                objective: "Collect inputs.".to_string(),
                depends_on: vec![],
                agent_role: "planner".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: Some(json!({
                    "builder": {
                        "output_path": ".tandem/artifacts/collect-inputs.json",
                        "output_files": ["custom/inputs.json"]
                    }
                })),
            },
            WorkflowPlanStep {
                step_id: "draft_report".to_string(),
                kind: "write".to_string(),
                objective: "Draft the report.".to_string(),
                depends_on: vec!["collect_inputs".to_string()],
                agent_role: "writer".to_string(),
                input_refs: vec![json!({
                    "from_step_id": "collect_inputs",
                    "alias": "inputs"
                })],
                output_contract: Some(json!({"kind":"report_markdown"})),
                metadata: Some(json!({
                    "builder": {
                        "input_files": ["docs/brief.md"]
                    }
                })),
            },
        ]);

        derive_workflow_step_file_contracts(&mut plan);

        let collect_builder = plan.steps[0]
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("collect builder");
        assert_eq!(
            collect_builder
                .get("output_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec!["custom/inputs.json"])
        );
        let draft_builder = plan.steps[1]
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("draft builder");
        assert_eq!(
            draft_builder
                .get("input_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec!["docs/brief.md"])
        );
    }

    #[test]
    fn validate_workflow_plan_accepts_supported_step_id_suffix_variants() {
        let plan = test_plan_with_steps(vec![
            WorkflowPlanStep {
                step_id: "research_sources_web".to_string(),
                kind: "research".to_string(),
                objective: "Research sources from the web.".to_string(),
                depends_on: vec![],
                agent_role: "researcher".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            },
            WorkflowPlanStep {
                step_id: "analyze_findings".to_string(),
                kind: "analysis".to_string(),
                objective: "Analyze findings.".to_string(),
                depends_on: vec!["research_sources_web".to_string()],
                agent_role: "analyst".to_string(),
                input_refs: vec![json!({
                    "from_step_id": "research_sources_web",
                    "alias": "source_data"
                })],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            },
        ]);

        validate_workflow_plan(&plan).expect("step-id suffix variants should be accepted");
    }

    #[test]
    fn validate_workflow_plan_rejects_unknown_step_ids() {
        let plan = test_plan_with_steps(vec![WorkflowPlanStep {
            step_id: "totally_custom_step".to_string(),
            kind: "custom".to_string(),
            objective: "Do custom work.".to_string(),
            depends_on: vec![],
            agent_role: "worker".to_string(),
            input_refs: vec![],
            output_contract: Some(json!({"kind":"structured_json"})),
            metadata: None,
        }]);

        let error = validate_workflow_plan(&plan).expect_err("unknown step id should fail");
        assert!(error.contains("unsupported workflow step id"));
    }

    #[test]
    fn infer_explicit_output_targets_extracts_path_like_workspace_targets() {
        let prompt = "Generate and save /home/user/marketing-tandem/YOUTUBE_TANDEM_MARKETING_RESEARCH_AND_SCRIPTS.md and also summarize the findings.";

        let targets = infer_explicit_output_targets(prompt);

        assert_eq!(
            targets,
            vec![
                "/home/user/marketing-tandem/YOUTUBE_TANDEM_MARKETING_RESEARCH_AND_SCRIPTS.md"
                    .to_string()
            ]
        );
    }

    #[test]
    fn infer_explicit_output_targets_ignores_urls_and_deduplicates_targets() {
        let prompt =
            "Write to https://example.com/report.md, ./notes/final.md, and ./notes/final.md again.";

        let targets = infer_explicit_output_targets(prompt);

        assert_eq!(targets, vec!["./notes/final.md".to_string()]);
    }

    #[test]
    fn workflow_plan_should_surface_mcp_discovery_for_connector_backed_sources() {
        assert!(workflow_plan_should_surface_mcp_discovery(
            "Research Reddit threads about AI assistants.",
            &[]
        ));
        assert!(workflow_plan_should_surface_mcp_discovery(
            "Write the workflow plan.",
            &["github".to_string()]
        ));
        assert!(!workflow_plan_should_surface_mcp_discovery(
            "Summarize the local workspace docs.",
            &[]
        ));
    }
}
