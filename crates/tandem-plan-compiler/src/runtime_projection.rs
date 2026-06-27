// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::{json, Map, Value};
use tandem_workflows::plan_package::{WorkflowPlan, WorkflowPlanStep};

use crate::automation_projection::{
    ProjectedAutomationAgentProfile, ProjectedAutomationDraft, ProjectedAutomationExecutionPolicy,
    ProjectedAutomationNode,
};
use crate::materialization::{
    materialization_seed_from_projection, ProjectedAutomationMaterializationSeed,
};
use crate::plan_package::PartialFailureMode;
use crate::workflow_plan::{
    agent_id_for_role, compile_operator_model_policy, compile_workflow_agent_tool_allowlist,
    display_name_for_role, infer_explicit_output_targets, plan_max_parallel_agents,
    workflow_plan_agent_roles,
};

pub fn compile_workflow_runtime_projection<S, I, O>(
    plan: &WorkflowPlan<S, WorkflowPlanStep<I, O>>,
    normalize_allowed_tools: impl Fn(Vec<String>) -> Vec<String>,
) -> ProjectedAutomationDraft<I, O>
where
    I: Clone,
    O: Clone,
{
    let model_policy = compile_operator_model_policy(plan.operator_preferences.as_ref());
    let tool_allowlist = compile_workflow_agent_tool_allowlist(
        &plan.allowed_mcp_servers,
        plan.operator_preferences.as_ref(),
        normalize_allowed_tools,
    );
    let agents = workflow_plan_agent_roles(&plan.steps, |step| step.agent_role.as_str())
        .into_iter()
        .map(|agent_role| ProjectedAutomationAgentProfile {
            agent_id: agent_id_for_role(&agent_role),
            template_id: None,
            display_name: display_name_for_role(&agent_role),
            model_policy: model_policy.clone(),
            tool_allowlist: tool_allowlist.clone(),
            allowed_mcp_servers: plan.allowed_mcp_servers.clone(),
        })
        .collect::<Vec<_>>();

    let fintech_strict = plan_is_fintech_compliance_risk_brief(plan);
    let nodes = plan
        .steps
        .iter()
        .map(|step| ProjectedAutomationNode {
            node_id: step.step_id.clone(),
            agent_id: agent_id_for_role(&step.agent_role),
            objective: normalize_projected_objective_json_escapes(&step.objective),
            depends_on: step.depends_on.clone(),
            input_refs: step.input_refs.clone(),
            output_contract: step.output_contract.clone(),
            retry_policy: Some(json!({
                "max_attempts": 3
            })),
            timeout_ms: workflow_runtime_step_timeout_ms(step),
            stage_kind: None,
            gate: None,
            partial_failure_mode: Some(PartialFailureMode::PauseDownstreamOnly),
            metadata: stamp_fintech_step_metadata(step.metadata.clone(), step, fintech_strict),
        })
        .collect::<Vec<_>>();
    let mut metadata = json!({
        "workflow_plan_id": plan.plan_id,
        "workflow_plan_source": plan.plan_source,
        "workflow_plan_version": plan.planner_version,
    });
    if fintech_strict {
        stamp_fintech_workflow_metadata(&mut metadata);
    }

    ProjectedAutomationDraft {
        name: plan.title.clone(),
        description: plan.description.clone(),
        workspace_root: Some(plan.workspace_root.clone()),
        output_targets: infer_explicit_output_targets(&plan.original_prompt),
        agents,
        nodes,
        execution: ProjectedAutomationExecutionPolicy {
            max_parallel_agents: Some(plan_max_parallel_agents(plan.operator_preferences.as_ref())),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        context: None,
        metadata,
    }
}

fn normalize_projected_objective_json_escapes(objective: &str) -> String {
    if objective.contains("\\\"") && (objective.contains('{') || objective.contains('[')) {
        objective.replace("\\\"", "\"")
    } else {
        objective.to_string()
    }
}

fn plan_is_fintech_compliance_risk_brief<S, I, O>(
    plan: &WorkflowPlan<S, WorkflowPlanStep<I, O>>,
) -> bool {
    let mut text = String::new();
    text.push_str(&plan.original_prompt);
    text.push('\n');
    text.push_str(&plan.normalized_prompt);
    text.push('\n');
    text.push_str(&plan.title);
    if let Some(description) = &plan.description {
        text.push('\n');
        text.push_str(description);
    }
    for step in &plan.steps {
        text.push('\n');
        text.push_str(&step.step_id);
        text.push('\n');
        text.push_str(&step.kind);
        text.push('\n');
        text.push_str(&step.objective);
    }
    let lowered = text.to_ascii_lowercase();
    let fintech = contains_any(
        &lowered,
        &[
            "fintech",
            "financial technology",
            "banking",
            "payments",
            "payment",
            "card issuer",
            "broker dealer",
        ],
    );
    let compliance_or_risk = contains_any(
        &lowered,
        &[
            "compliance",
            "regulatory",
            "regulation",
            "risk",
            "controls",
            "control evidence",
            "aml",
            "kyc",
            "kyb",
        ],
    );
    let brief_artifact = contains_any(
        &lowered,
        &[
            "brief",
            "update brief",
            "risk update",
            "evidence packet",
            "review packet",
            "investigation summary",
            "exception report",
            "incident timeline",
        ],
    );
    fintech && compliance_or_risk && brief_artifact
}

fn stamp_fintech_workflow_metadata(metadata: &mut Value) {
    let Some(root) = ensure_object(metadata) else {
        return;
    };
    root.entry("runtime_profile".to_string())
        .or_insert_with(|| Value::String(tandem_core::FINTECH_STRICT_PROFILE.to_string()));
    root.entry("domain_profile".to_string())
        .or_insert_with(|| Value::String(tandem_core::FINTECH_STRICT_PROFILE.to_string()));
    root.entry("fintech_profile".to_string())
        .or_insert_with(|| Value::String(tandem_core::FINTECH_STRICT_PROFILE.to_string()));
    root.entry("fintech_strict".to_string())
        .or_insert(Value::Bool(true));
}

fn stamp_fintech_step_metadata<I, O>(
    metadata: Option<Value>,
    step: &WorkflowPlanStep<I, O>,
    fintech_strict: bool,
) -> Option<Value> {
    if !fintech_strict || !step_is_fintech_brief_artifact(step) {
        return metadata;
    }
    let mut metadata = metadata.unwrap_or_else(|| json!({}));
    let Some(root) = ensure_object(&mut metadata) else {
        return Some(metadata);
    };
    root.entry("fintech_strict".to_string())
        .or_insert(Value::Bool(true));
    root.entry("artifact_contract".to_string())
        .or_insert_with(|| Value::String("compliance_risk_update_brief".to_string()));
    let fintech = root
        .entry("fintech".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(fintech) = fintech.as_object_mut() {
        fintech
            .entry("artifact_contract".to_string())
            .or_insert_with(|| Value::String("compliance_risk_update_brief".to_string()));
        fintech
            .entry("strict_profile".to_string())
            .or_insert(Value::Bool(true));
    }
    Some(metadata)
}

fn step_is_fintech_brief_artifact<I, O>(step: &WorkflowPlanStep<I, O>) -> bool {
    let lowered =
        format!("{}\n{}\n{}", step.step_id, step.kind, step.objective).to_ascii_lowercase();
    contains_any(
        &lowered,
        &[
            "brief",
            "risk update",
            "evidence packet",
            "review packet",
            "investigation summary",
            "exception report",
            "incident timeline",
            "summar",
            "synthes",
            "final",
            "deliverable",
            "report",
        ],
    ) && contains_any(
        &lowered,
        &[
            "compliance",
            "regulatory",
            "risk",
            "control",
            "evidence",
            "aml",
            "kyc",
            "kyb",
        ],
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn ensure_object(value: &mut Value) -> Option<&mut Map<String, Value>> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut()
}

fn workflow_runtime_step_timeout_ms<I, O>(step: &WorkflowPlanStep<I, O>) -> Option<u64> {
    let step_id = step.step_id.trim().to_ascii_lowercase();
    let kind = step.kind.trim().to_ascii_lowercase();
    if step_id == "execute_goal" || kind == "execute" {
        Some(1_800_000)
    } else {
        None
    }
}

pub fn project_workflow_runtime_materialization_seed<S, I, O>(
    plan: &WorkflowPlan<S, WorkflowPlanStep<I, O>>,
    normalize_allowed_tools: impl Fn(Vec<String>) -> Vec<String>,
) -> ProjectedAutomationMaterializationSeed<I, O>
where
    I: Clone,
    O: Clone,
{
    materialization_seed_from_projection(compile_workflow_runtime_projection(
        plan,
        normalize_allowed_tools,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tandem_workflows::plan_package::{
        AutomationV2Schedule, AutomationV2ScheduleType, WorkflowPlan,
    };

    fn test_plan() -> WorkflowPlan<AutomationV2Schedule<Value>, WorkflowPlanStep<Value, Value>> {
        WorkflowPlan {
            plan_id: "wfplan-test".to_string(),
            planner_version: "v1".to_string(),
            plan_source: "unit_test".to_string(),
            original_prompt: "test prompt".to_string(),
            normalized_prompt: "test prompt".to_string(),
            confidence: "medium".to_string(),
            title: "Runtime Test".to_string(),
            description: Some("desc".to_string()),
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: Value::String("run_once".to_string()),
            },
            execution_target: "automation_v2".to_string(),
            workspace_root: "/tmp/project".to_string(),
            steps: vec![WorkflowPlanStep {
                step_id: "execute_goal".to_string(),
                kind: "execute".to_string(),
                objective: "Do the thing".to_string(),
                depends_on: Vec::new(),
                agent_role: "worker".to_string(),
                input_refs: Vec::new(),
                output_contract: Some(json!({"kind": "structured_json"})),
                metadata: Some(json!({"phase": "main"})),
            }],
            requires_integrations: Vec::new(),
            allowed_mcp_servers: vec!["github".to_string()],
            operator_preferences: Some(json!({
                "model_provider": "test-provider",
                "model_id": "test-model"
            })),
            save_options: json!({}),
        }
    }

    #[test]
    fn compile_workflow_runtime_projection_shapes_agents_and_nodes() {
        let projection = compile_workflow_runtime_projection(&test_plan(), |allowlist| allowlist);

        assert_eq!(projection.agents.len(), 1);
        assert_eq!(projection.agents[0].agent_id, "agent_worker");
        assert_eq!(projection.nodes.len(), 1);
        assert_eq!(projection.nodes[0].node_id, "execute_goal");
        assert_eq!(projection.nodes[0].timeout_ms, Some(1_800_000));
        assert_eq!(
            projection.nodes[0].partial_failure_mode,
            Some(PartialFailureMode::PauseDownstreamOnly)
        );
        assert_eq!(projection.execution.max_parallel_agents, Some(1));
        assert_eq!(projection.name, "Runtime Test");
        assert_eq!(
            projection
                .metadata
                .get("workflow_plan_id")
                .and_then(Value::as_str),
            Some("wfplan-test")
        );
        assert!(projection.output_targets.is_empty());
    }

    #[test]
    fn compile_workflow_runtime_projection_preserves_connector_writer_metadata() {
        let mut plan = test_plan();
        plan.steps[0].metadata = Some(json!({
            "connector": "notion",
            "connector_writer": true,
            "writer_kind": "database_rows",
            "notion_data_source_url": "collection://ds-123",
            "input_alias": "filtered_rows",
            "source_array_key": "rows",
            "title_property": "Name",
            "property_mappings": {
                "Name": "name",
                "URL": "url"
            },
            "duplicate_keys": ["URL"]
        }));

        let projection = compile_workflow_runtime_projection(&plan, |allowlist| allowlist);
        let metadata = projection.nodes[0]
            .metadata
            .as_ref()
            .expect("node metadata");

        assert_eq!(
            metadata.get("connector").and_then(Value::as_str),
            Some("notion")
        );
        assert_eq!(
            metadata.get("connector_writer").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            metadata
                .get("property_mappings")
                .and_then(Value::as_object)
                .and_then(|mappings| mappings.get("URL"))
                .and_then(Value::as_str),
            Some("url")
        );
    }

    #[test]
    fn compile_workflow_runtime_projection_normalizes_escaped_json_objective_keys() {
        let mut plan = test_plan();
        plan.steps[0].objective =
            "Call tool with {\"tools\":[{\"arguments\":{\"query\":\"reddit\",\\\"limit\\\":10}}]}"
                .to_string();

        let projection = compile_workflow_runtime_projection(&plan, |allowlist| allowlist);

        assert!(projection.nodes[0].objective.contains("\"limit\":10"));
        assert!(!projection.nodes[0].objective.contains("\\\"limit\\\""));
    }

    #[test]
    fn compile_workflow_runtime_projection_stamps_fintech_brief_profile() {
        let mut plan = test_plan();
        plan.original_prompt =
            "Create a fintech compliance and risk update brief for new payment regulations."
                .to_string();
        plan.normalized_prompt =
            "create fintech compliance risk update brief for payment regulations".to_string();
        plan.title = "Fintech compliance risk brief".to_string();
        plan.steps[0].step_id = "draft_compliance_brief".to_string();
        plan.steps[0].kind = "draft".to_string();
        plan.steps[0].objective =
            "Draft the compliance and risk update brief with cited source evidence.".to_string();
        plan.steps[0].metadata = None;

        let projection = compile_workflow_runtime_projection(&plan, |allowlist| allowlist);

        assert_eq!(
            projection
                .metadata
                .get("runtime_profile")
                .and_then(Value::as_str),
            Some(tandem_core::FINTECH_STRICT_PROFILE)
        );
        assert!(tandem_core::metadata_enables_fintech_strict(Some(
            &projection.metadata
        )));
        let node_metadata = projection.nodes[0]
            .metadata
            .as_ref()
            .expect("node metadata");
        assert_eq!(
            node_metadata
                .get("artifact_contract")
                .and_then(Value::as_str),
            Some("compliance_risk_update_brief")
        );
        assert_eq!(
            node_metadata
                .get("fintech")
                .and_then(|value| value.get("artifact_contract"))
                .and_then(Value::as_str),
            Some("compliance_risk_update_brief")
        );
    }

    #[test]
    fn compile_workflow_runtime_projection_does_not_stamp_generic_finance_workflow() {
        let mut plan = test_plan();
        plan.original_prompt =
            "Research finance newsletter topics and summarize likely reader interest.".to_string();
        plan.normalized_prompt = "research finance newsletter topics".to_string();
        plan.title = "Finance newsletter research".to_string();
        plan.steps[0].objective =
            "Summarize market newsletter topic ideas for editorial review.".to_string();

        let projection = compile_workflow_runtime_projection(&plan, |allowlist| allowlist);

        assert!(projection.metadata.get("runtime_profile").is_none());
        assert!(!tandem_core::metadata_enables_fintech_strict(Some(
            &projection.metadata
        )));
        assert_eq!(
            projection.nodes[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("artifact_contract"))
                .and_then(Value::as_str),
            None
        );
    }
}
