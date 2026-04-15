// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::ModelSpec;
use tandem_workflows::plan_package::{AutomationV2Schedule, WorkflowPlan, WorkflowPlanStep};

use crate::decomposition::{
    derive_workflow_decomposition_profile, workflow_plan_decomposition_observation,
    workflow_plan_decomposition_sections,
};
use crate::host::{PlannerLlmInvocation, PlannerLoopHost, WorkspaceResolver};
use crate::planner_invoke::invoke_planner_json;
use crate::planner_prompts::workflow_plan_common_sections;
use crate::planner_types::{PlannerClarifier, PlannerInvocationFailure};
use crate::workflow_plan::{
    build_minimal_fallback_plan, decode_planner_plan_value, infer_explicit_output_targets,
    manual_schedule, normalize_and_validate_planner_plan, normalize_operator_preferences,
    normalize_prompt, normalize_string_list, plan_save_options, plan_title, planner_diagnostics,
    planner_llm_provider_unconfigured_hint, planner_model_spec, schedule_from_value, truncate_text,
    workflow_plan_should_surface_mcp_discovery, PlannerPlanMode, PlannerPlanNormalizationContext,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerBuildConfig {
    pub session_title: String,
    pub timeout_ms: u64,
    pub override_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerBuildRequest<M> {
    pub plan_id: String,
    pub planner_version: String,
    pub plan_source: String,
    pub prompt: String,
    pub normalized_prompt: String,
    pub title: String,
    pub fallback_schedule: AutomationV2Schedule<M>,
    pub explicit_schedule: Option<AutomationV2Schedule<M>>,
    pub requested_workspace_root: Option<String>,
    pub allowed_mcp_servers: Vec<String>,
    pub operator_preferences: Option<Value>,
}

pub fn prepare_build_request<M>(
    plan_id: String,
    planner_version: String,
    plan_source: String,
    prompt: &str,
    explicit_schedule: Option<&Value>,
    default_timezone: &str,
    default_misfire_policy: M,
    allowed_mcp_servers: Vec<String>,
    requested_workspace_root: Option<&str>,
    operator_preferences: Option<Value>,
) -> PlannerBuildRequest<M>
where
    M: Clone,
{
    let normalized_prompt = normalize_prompt(prompt);
    let explicit_schedule = explicit_schedule
        .and_then(|value| schedule_from_value(value, default_misfire_policy.clone()));
    let fallback_schedule = explicit_schedule
        .clone()
        .unwrap_or_else(|| manual_schedule(default_timezone.to_string(), default_misfire_policy));
    let title = plan_title(prompt, &fallback_schedule.schedule_type);

    PlannerBuildRequest {
        plan_id,
        planner_version,
        plan_source,
        prompt: prompt.trim().to_string(),
        normalized_prompt,
        title,
        fallback_schedule,
        explicit_schedule,
        requested_workspace_root: requested_workspace_root
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string()),
        allowed_mcp_servers: normalize_string_list(allowed_mcp_servers),
        operator_preferences: normalize_operator_preferences(operator_preferences),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerBuildResult<M, I, O>
where
    I: Default,
    O: Default,
{
    pub plan: WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    pub assistant_text: Option<String>,
    pub clarifier: Value,
    pub planner_diagnostics: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PlannerBuildAction {
    Build,
    Clarify,
}

#[derive(Debug, Deserialize)]
struct PlannerBuildPayload {
    action: PlannerBuildAction,
    #[serde(default)]
    assistant_text: Option<String>,
    #[serde(default)]
    clarifier: Option<PlannerClarifier>,
    #[serde(default)]
    #[serde(alias = "workflow_plan")]
    plan: Option<Value>,
}

pub async fn build_workflow_plan_with_planner<M, I, O, H>(
    host: &H,
    request: PlannerBuildRequest<M>,
    config: PlannerBuildConfig,
    mut normalize_step: impl FnMut(&mut WorkflowPlanStep<I, O>),
    fallback_step: WorkflowPlanStep<I, O>,
) -> PlannerBuildResult<M, I, O>
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone
        + Default
        + crate::workflow_plan::WorkflowInputRefLike
        + serde::Serialize
        + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
    H: PlannerLoopHost + WorkspaceResolver,
{
    let explicit_output_targets = infer_explicit_output_targets(&request.prompt);
    let decomposition_profile = derive_workflow_decomposition_profile(
        &request.prompt,
        &request.allowed_mcp_servers,
        &explicit_output_targets,
        request.explicit_schedule.is_some(),
    );
    let resolved_workspace_root = match host
        .resolve_workspace_root(request.requested_workspace_root.as_deref())
        .await
    {
        Ok(root) => root,
        Err(error) => {
            return PlannerBuildResult {
                plan: build_minimal_fallback_plan(
                    &request.plan_id,
                    &request.planner_version,
                    &request.plan_source,
                    &request.prompt,
                    &request.normalized_prompt,
                    request.title,
                    request
                        .requested_workspace_root
                        .unwrap_or_else(|| "/".to_string()),
                    request.fallback_schedule,
                    request.allowed_mcp_servers,
                    request.operator_preferences,
                    Some(format!(
                        "Planner fallback draft. Invalid workspace root: {}",
                        truncate_text(&error, 200)
                    )),
                    fallback_step.clone(),
                ),
                assistant_text: Some(error),
                clarifier: json!({
                    "field": "workspace_root",
                    "question": "The requested workspace root is invalid. Update it and try again.",
                    "options": [],
                }),
                planner_diagnostics: planner_diagnostics(
                    Some("invalid_workspace_root"),
                    None,
                    Some(workflow_plan_decomposition_observation(
                        &decomposition_profile,
                        0,
                    )),
                ),
            };
        }
    };

    let normalization_ctx = PlannerPlanNormalizationContext {
        mode: PlannerPlanMode::Create,
        plan_id: &request.plan_id,
        planner_version: &request.planner_version,
        plan_source: &request.plan_source,
        original_prompt: request.prompt.trim(),
        normalized_prompt: &request.normalized_prompt,
        resolved_workspace_root: &resolved_workspace_root,
        explicit_schedule: request.explicit_schedule.as_ref(),
        request_allowed_mcp_servers: &request.allowed_mcp_servers,
        request_operator_preferences: request.operator_preferences.as_ref(),
    };

    let Some(model) = planner_model_spec(request.operator_preferences.as_ref()) else {
        return PlannerBuildResult {
            plan: build_minimal_fallback_plan(
                &request.plan_id,
                &request.planner_version,
                &request.plan_source,
                &request.prompt,
                &request.normalized_prompt,
                request.title,
                resolved_workspace_root,
                request.fallback_schedule,
                request.allowed_mcp_servers,
                request.operator_preferences,
                Some(
                    "Planner fallback draft. Configure a planner model for richer workflow planning. Reason: no_planner_model."
                        .to_string(),
                ),
                fallback_step.clone(),
            ),
            assistant_text: None,
            clarifier: Value::Null,
            planner_diagnostics: planner_diagnostics(
                Some("no_planner_model"),
                None,
                Some(workflow_plan_decomposition_observation(
                    &decomposition_profile,
                    0,
                )),
            ),
        };
    };

    if !host.is_provider_configured(&model.provider_id).await {
        let question = planner_llm_provider_unconfigured_hint(&model.provider_id);
        return PlannerBuildResult {
            plan: build_minimal_fallback_plan(
                &request.plan_id,
                &request.planner_version,
                &request.plan_source,
                &request.prompt,
                &request.normalized_prompt,
                request.title,
                resolved_workspace_root,
                request.fallback_schedule,
                request.allowed_mcp_servers,
                request.operator_preferences,
                Some(
                    "Planner fallback draft. Configure the planner provider for richer workflow generation. Reason: provider_unconfigured."
                        .to_string(),
                ),
                fallback_step.clone(),
            ),
            assistant_text: Some(question.clone()),
            clarifier: json!({
                "field": "general",
                "question": question,
                "options": [],
            }),
            planner_diagnostics: planner_diagnostics(
                Some("provider_unconfigured"),
                None,
                Some(workflow_plan_decomposition_observation(
                    &decomposition_profile,
                    0,
                )),
            ),
        };
    }

    match try_llm_build_workflow_plan(
        host,
        &config,
        &model,
        request.prompt.as_str(),
        request.normalized_prompt.as_str(),
        request.explicit_schedule.as_ref(),
        request.plan_source.as_str(),
        resolved_workspace_root.as_str(),
        &request.allowed_mcp_servers,
        request.operator_preferences.as_ref(),
        &decomposition_profile,
    )
    .await
    {
        Ok(payload) => match payload.action {
            PlannerBuildAction::Build => {
                let Some(candidate) = payload.plan.and_then(|plan| {
                    decode_build_plan_candidate(
                        plan,
                        &request,
                        resolved_workspace_root.as_str(),
                    )
                }) else {
                    return PlannerBuildResult {
                        plan: build_minimal_fallback_plan(
                            &request.plan_id,
                            &request.planner_version,
                            &request.plan_source,
                            &request.prompt,
                            &request.normalized_prompt,
                            request.title,
                            resolved_workspace_root,
                            request.fallback_schedule,
                            request.allowed_mcp_servers,
                            request.operator_preferences,
                            Some(
                                "Planner fallback draft. The planner returned an invalid JSON plan. Reason: invalid_json."
                                    .to_string(),
                            ),
                            fallback_step.clone(),
                        ),
                        assistant_text: payload.assistant_text.or(Some(
                            "The planner returned a response Tandem could not parse into a valid workflow plan."
                                .to_string(),
                        )),
                        clarifier: Value::Null,
                        planner_diagnostics: planner_diagnostics(
                            Some("invalid_json"),
                            None,
                            Some(workflow_plan_decomposition_observation(
                                &decomposition_profile,
                                0,
                            )),
                        ),
                    };
                };

                let candidate_step_count = candidate.steps.len();
                match normalize_and_validate_planner_plan(
                    candidate,
                    &normalization_ctx,
                    &mut normalize_step,
                ) {
                    Ok(plan) => {
                        let diagnostics = planner_diagnostics(
                            None,
                            None,
                            Some(workflow_plan_decomposition_observation(
                                &decomposition_profile,
                                plan.steps.len(),
                            )),
                        );
                        PlannerBuildResult {
                            plan,
                            assistant_text: payload.assistant_text,
                            clarifier: Value::Null,
                            planner_diagnostics: diagnostics,
                        }
                    },
                    Err(error) => {
                        let detail = truncate_text(&error, 500);
                        host.warn(&format!(
                            "workflow planner llm output rejected by validation: {detail}"
                        ));
                        PlannerBuildResult {
                            plan: build_minimal_fallback_plan(
                                &request.plan_id,
                                &request.planner_version,
                                &request.plan_source,
                                &request.prompt,
                                &request.normalized_prompt,
                                request.title,
                                resolved_workspace_root,
                            request.fallback_schedule,
                            request.allowed_mcp_servers,
                            request.operator_preferences,
                            Some("Planner fallback draft. The planner returned a workflow that Tandem could not validate. Reason: validation_rejected.".to_string()),
                                fallback_step.clone(),
                            ),
                            assistant_text: payload.assistant_text.or(Some(
                                "The planner returned a workflow Tandem could not validate. Tandem used a minimal fallback plan instead.".to_string(),
                            )),
                            clarifier: Value::Null,
                            planner_diagnostics: planner_diagnostics(
                                Some("validation_rejected"),
                                Some(detail),
                                Some(workflow_plan_decomposition_observation(
                                    &decomposition_profile,
                                    candidate_step_count,
                                )),
                            ),
                        }
                    }
                }
            }
            PlannerBuildAction::Clarify => {
                let question = payload
                    .clarifier
                    .as_ref()
                    .map(|row| row.question.trim())
                    .filter(|value| !value.is_empty())
                    .unwrap_or("The request is ambiguous. Clarify the workflow goal or constraints.");
                let field = payload
                    .clarifier
                    .as_ref()
                    .and_then(|row| row.field.as_deref())
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("general");
                PlannerBuildResult {
                    plan: build_minimal_fallback_plan(
                        &request.plan_id,
                        &request.planner_version,
                        &request.plan_source,
                        &request.prompt,
                        &request.normalized_prompt,
                        request.title,
                        resolved_workspace_root,
                        request.fallback_schedule,
                        request.allowed_mcp_servers,
                        request.operator_preferences,
                        Some("Planner fallback draft. Clarification is needed before Tandem can generate a richer workflow. Reason: clarification_needed.".to_string()),
                        fallback_step.clone(),
                    ),
                    assistant_text: Some(payload.assistant_text.unwrap_or_else(|| question.to_string())),
                    clarifier: json!({
                        "field": field,
                        "question": question,
                        "options": [],
                    }),
                    planner_diagnostics: planner_diagnostics(
                        Some("clarification_needed"),
                        None,
                        Some(workflow_plan_decomposition_observation(
                            &decomposition_profile,
                            0,
                        )),
                    ),
                }
            }
        },
        Err(failure) => PlannerBuildResult {
            plan: build_minimal_fallback_plan(
                &request.plan_id,
                &request.planner_version,
                &request.plan_source,
                request.prompt.as_str(),
                &request.normalized_prompt,
                request.title,
                resolved_workspace_root,
                request.fallback_schedule,
                request.allowed_mcp_servers,
                request.operator_preferences,
                Some(format!(
                    "Planner fallback draft. Tandem could not complete a provider-safe planning call for this model. Reason: {}.",
                    failure.reason
                )),
                fallback_step.clone(),
            ),
            assistant_text: Some(
                failure.detail.clone().unwrap_or_else(|| {
                    "The planner could not complete a valid provider call. Tandem used a minimal fallback workflow instead."
                        .to_string()
                }),
            ),
            clarifier: Value::Null,
            planner_diagnostics: planner_diagnostics(
                Some(failure.reason.as_str()),
                failure.detail,
                Some(workflow_plan_decomposition_observation(
                    &decomposition_profile,
                    0,
                )),
            ),
        },
    }
}

fn decode_build_plan_candidate<M, I, O>(
    mut plan: Value,
    request: &PlannerBuildRequest<M>,
    resolved_workspace_root: &str,
) -> Option<WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>>
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone
        + Default
        + crate::workflow_plan::WorkflowInputRefLike
        + serde::Serialize
        + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
{
    if let Some(decoded) = decode_planner_plan_value(plan.clone()) {
        return Some(decoded);
    }
    let Some(map) = plan.as_object_mut() else {
        return None;
    };
    let schedule_value = serde_json::to_value(
        request
            .explicit_schedule
            .clone()
            .unwrap_or_else(|| request.fallback_schedule.clone()),
    )
    .unwrap_or(Value::Null);
    map.entry("plan_id".to_string())
        .or_insert_with(|| Value::String(request.plan_id.clone()));
    map.entry("planner_version".to_string())
        .or_insert_with(|| Value::String(request.planner_version.clone()));
    map.entry("plan_source".to_string())
        .or_insert_with(|| Value::String(request.plan_source.clone()));
    map.entry("original_prompt".to_string())
        .or_insert_with(|| Value::String(request.prompt.clone()));
    map.entry("normalized_prompt".to_string())
        .or_insert_with(|| Value::String(request.normalized_prompt.clone()));
    map.entry("confidence".to_string())
        .or_insert_with(|| Value::String("medium".to_string()));
    map.entry("title".to_string())
        .or_insert_with(|| Value::String(request.title.clone()));
    map.entry("schedule".to_string())
        .or_insert_with(|| schedule_value);
    map.entry("execution_target".to_string())
        .or_insert_with(|| Value::String("automation_v2".to_string()));
    map.entry("workspace_root".to_string())
        .or_insert_with(|| Value::String(resolved_workspace_root.to_string()));
    map.entry("requires_integrations".to_string())
        .or_insert_with(|| json!([]));
    map.entry("allowed_mcp_servers".to_string())
        .or_insert_with(|| json!(request.allowed_mcp_servers.clone()));
    if let Some(operator_preferences) = request.operator_preferences.clone() {
        map.entry("operator_preferences".to_string())
            .or_insert(operator_preferences);
    }
    map.entry("save_options".to_string())
        .or_insert_with(plan_save_options);
    decode_planner_plan_value(plan)
}

async fn try_llm_build_workflow_plan<M, H>(
    host: &H,
    config: &PlannerBuildConfig,
    model: &ModelSpec,
    prompt: &str,
    normalized_prompt: &str,
    explicit_schedule: Option<&AutomationV2Schedule<M>>,
    plan_source: &str,
    workspace_root: &str,
    allowed_mcp_servers: &[String],
    operator_preferences: Option<&Value>,
    decomposition_profile: &crate::decomposition::WorkflowDecompositionProfile,
) -> Result<PlannerBuildPayload, PlannerInvocationFailure>
where
    M: serde::Serialize,
    H: PlannerLoopHost,
{
    let capability_summary = host.capability_summary(allowed_mcp_servers).await;
    invoke_planner_json(
        host,
        PlannerLlmInvocation {
            session_title: config.session_title.clone(),
            workspace_root: workspace_root.to_string(),
            model: model.clone(),
            prompt: build_llm_workflow_creation_prompt(
                prompt,
                normalized_prompt,
                explicit_schedule,
                plan_source,
                workspace_root,
                allowed_mcp_servers,
                operator_preferences,
                &capability_summary,
                decomposition_profile,
            ),
            run_key: format!("workflow-plan-build:{plan_source}"),
            timeout_ms: config.timeout_ms,
            override_env: config.override_env.clone(),
        },
    )
    .await
}

fn build_llm_workflow_creation_prompt<M: serde::Serialize>(
    prompt: &str,
    normalized_prompt: &str,
    explicit_schedule: Option<&AutomationV2Schedule<M>>,
    plan_source: &str,
    workspace_root: &str,
    allowed_mcp_servers: &[String],
    operator_preferences: Option<&Value>,
    capability_summary: &Value,
    decomposition_profile: &crate::decomposition::WorkflowDecompositionProfile,
) -> String {
    let common_sections = workflow_plan_common_sections();
    let decomposition_sections = workflow_plan_decomposition_sections(decomposition_profile);
    let mcp_discovery_required =
        workflow_plan_should_surface_mcp_discovery(prompt, allowed_mcp_servers);
    let mcp_guidance = if mcp_discovery_required {
        format!(
            "MCP discovery:\n- Use the planner capability summary and runtime MCP inventory before inventing tools or falling back to generic web search.\n- Call `mcp_list` when you need to confirm which MCP tools are available.\n- If the request names connector-backed sources such as Reddit, GitHub issues, Slack, or Jira, plan MCP-backed steps when a relevant server is available.\n- If the request depends on a connector-backed source but no relevant MCP path is available, return a clarifier instead of guessing.\n- Allowed MCP servers: {}\n",
            serde_json::to_string_pretty(&allowed_mcp_servers).unwrap_or_else(|_| "[]".to_string())
        )
    } else {
        String::new()
    };
    format!(
        concat!(
            "You are creating a Tandem automation workflow plan.\n",
            "Planner intelligence lives in the model. Return JSON only.\n",
            "{}",
            "{}",
            "- include output_contract validators only when you are confident of the artifact kind\n",
            "Request context:\n",
            "- workspace_root: {}\n",
            "- plan_source: {}\n",
            "- explicit_schedule: {}\n",
            "- allowed_mcp_servers: {}\n",
            "- operator_preferences: {}\n",
            "{}",
            "Planner capability summary and runtime MCP inventory (use this instead of inventing tools or hidden capabilities):\n{}\n",
            "Delivery rule:\n",
            "- plan email delivery only when the capability summary shows email_send or email_draft\n",
            "- default email delivery to inline body content\n",
            "- only plan an attachment when a workflow step is expected to produce a concrete attachment artifact such as an upload result or valid s3key\n",
            "Return one of:\n",
            "{{\"action\":\"build\",\"assistant_text\":\"...\",\"plan\":{{...full WorkflowPlan...}}}}\n",
            "{{\"action\":\"clarify\",\"assistant_text\":\"...\",\"clarifier\":{{\"field\":\"general\",\"question\":\"...\"}}}}\n",
            "Original prompt:\n{}\n\n",
            "Normalized prompt:\n{}\n"
        ),
        common_sections,
        decomposition_sections,
        workspace_root,
        plan_source,
        serde_json::to_string_pretty(&explicit_schedule).unwrap_or_else(|_| "null".to_string()),
        serde_json::to_string_pretty(&allowed_mcp_servers).unwrap_or_else(|_| "[]".to_string()),
        serde_json::to_string_pretty(&operator_preferences).unwrap_or_else(|_| "null".to_string()),
        mcp_guidance,
        serde_json::to_string_pretty(capability_summary).unwrap_or_else(|_| "{}".to_string()),
        prompt.trim(),
        normalized_prompt,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tandem_workflows::plan_package::AutomationV2ScheduleType;

    #[test]
    fn prepare_build_request_normalizes_and_defaults_fields() {
        let request = prepare_build_request(
            "wfplan-test".to_string(),
            "v1".to_string(),
            "unit_test".to_string(),
            "  Build a workflow for me  ",
            None,
            "UTC",
            Value::String("run_once".to_string()),
            vec![" GitHub ".to_string(), "github".to_string(), "".to_string()],
            Some("  /tmp/project  "),
            Some(json!({
                "model_provider": " test-provider ",
                "model_id": " test-model "
            })),
        );

        assert_eq!(request.prompt, "Build a workflow for me");
        assert_eq!(request.normalized_prompt, "build a workflow for me");
        assert_eq!(request.title, "Build a workflow for me");
        assert_eq!(
            request.requested_workspace_root.as_deref(),
            Some("/tmp/project")
        );
        assert_eq!(
            request.allowed_mcp_servers,
            vec!["GitHub".to_string(), "github".to_string()]
        );
        assert_eq!(
            request.operator_preferences,
            Some(json!({
                "model_provider": "test-provider",
                "model_id": "test-model"
            }))
        );
        assert_eq!(
            request.fallback_schedule.schedule_type,
            AutomationV2ScheduleType::Manual
        );
        assert!(request.explicit_schedule.is_none());
    }

    #[test]
    fn decode_build_plan_candidate_backfills_missing_required_fields() {
        let request = prepare_build_request(
            "wfplan-partial".to_string(),
            "v1".to_string(),
            "unit_test".to_string(),
            "Create a workflow",
            None::<&Value>,
            "UTC",
            Value::String("run_once".to_string()),
            vec!["composio-1".to_string()],
            Some("/tmp/workspace"),
            Some(json!({
                "model_provider": "openrouter",
                "model_id": "openai/gpt-5.4"
            })),
        );

        let partial_plan = json!({
            "title": "Planner Draft Title",
            "steps": [
                {
                    "step_id": "collect_inputs",
                    "kind": "research",
                    "objective": "Inspect workspace files first",
                    "agent_role": "Research Analyst"
                }
            ]
        });

        let decoded = decode_build_plan_candidate::<Value, Value, Value>(
            partial_plan,
            &request,
            "/tmp/workspace",
        )
        .expect("partial planner plan should decode after backfill");

        assert_eq!(decoded.plan_id, "wfplan-partial");
        assert_eq!(decoded.planner_version, "v1");
        assert_eq!(decoded.plan_source, "unit_test");
        assert_eq!(decoded.title, "Planner Draft Title");
        assert_eq!(decoded.execution_target, "automation_v2");
        assert_eq!(decoded.workspace_root, "/tmp/workspace");
        assert_eq!(decoded.allowed_mcp_servers, vec!["composio-1".to_string()]);
        assert_eq!(
            decoded.schedule.schedule_type,
            AutomationV2ScheduleType::Manual
        );
        assert_eq!(decoded.steps.len(), 1);
        assert_eq!(decoded.steps[0].step_id, "collect_inputs");
        assert!(decoded
            .save_options
            .get("can_export_pack")
            .and_then(Value::as_bool)
            .unwrap_or(false));
    }

    #[test]
    fn build_workflow_plan_prompt_surfaces_mcp_discovery_guidance() {
        let decomposition_profile = derive_workflow_decomposition_profile(
            "Create a workflow about Reddit research",
            &["github".to_string()],
            &[],
            false,
        );
        let prompt = build_llm_workflow_creation_prompt::<Value>(
            "Create a workflow about Reddit research",
            "Create a workflow about Reddit research",
            None,
            "unit_test",
            "/tmp/workspace",
            &["github".to_string()],
            None,
            &json!({
                "runtime": {"mcp_inventory": []}
            }),
            &decomposition_profile,
        );

        assert!(prompt.contains("MCP discovery:"));
        assert!(prompt.contains("Call `mcp_list`"));
        assert!(prompt.contains("Allowed MCP servers"));
        assert!(prompt.contains("Decomposition profile:"));
        assert!(prompt.contains("phase-aware microtask DAGs"));
    }
}
