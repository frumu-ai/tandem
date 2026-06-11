// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::ModelSpec;
use tandem_workflows::plan_package::{
    AutomationV2Schedule, AutomationV2ScheduleType, WorkflowPlan, WorkflowPlanConversation,
    WorkflowPlanStep,
};

use crate::decomposition::{
    derive_workflow_decomposition_profile, workflow_plan_decomposition_observation,
    workflow_plan_decomposition_sections,
};
use crate::host::{PlannerLlmInvocation, PlannerLoopHost};
use crate::planner_invoke::invoke_planner_json;
use crate::planner_messages::{
    planner_failure_clarifier_hint, planner_llm_invalid_response_hint, planner_llm_unavailable_hint,
};
use crate::planner_types::{PlannerClarifier, PlannerInvocationFailure};
use crate::workflow_plan::{
    compact_generated_workflow_plan_to_budget, decode_planner_plan_value,
    infer_explicit_output_targets, normalize_and_validate_planner_plan,
    planner_llm_provider_unconfigured_hint, planner_model_spec,
    workflow_plan_decomposition_observation_with_task_budget, workflow_schedule_equal,
    workflow_steps_equal, PlannerPlanMode, PlannerPlanNormalizationContext, WorkflowInputRefLike,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerRevisionAction {
    Revise,
    Clarify,
    Keep,
}

#[derive(Debug, Deserialize)]
pub struct PlannerRevisionPayload {
    pub action: PlannerRevisionAction,
    #[serde(default)]
    pub assistant_text: Option<String>,
    #[serde(default)]
    pub change_summary: Vec<String>,
    #[serde(default)]
    pub clarifier: Option<PlannerClarifier>,
    #[serde(default)]
    #[serde(alias = "workflow_plan")]
    pub plan: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerLoopConfig {
    pub session_title: String,
    pub timeout_ms: u64,
    pub override_env: String,
}

fn planner_revision_failure_clarifier(question: impl Into<String>, reason: &'static str) -> Value {
    json!({
        "field": "general",
        "question": question.into(),
        "options": [],
        "revision_failed": true,
        "blocks_activation": true,
        "failure_reason": reason,
    })
}

pub async fn revise_workflow_plan_with_planner_loop<M, I, O, H>(
    host: &H,
    current_plan: &WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    conversation: &WorkflowPlanConversation,
    message: &str,
    config: PlannerLoopConfig,
    mut normalize_step: impl FnMut(&mut WorkflowPlanStep<I, O>),
) -> (
    WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    String,
    Vec<String>,
    Value,
    Option<Value>,
)
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone + Default + WorkflowInputRefLike + serde::Serialize + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
    H: PlannerLoopHost,
{
    let decomposition_profile = derive_workflow_decomposition_profile(
        &current_plan.original_prompt,
        &current_plan.allowed_mcp_servers,
        &infer_explicit_output_targets(&current_plan.original_prompt),
        !matches!(
            &current_plan.schedule.schedule_type,
            AutomationV2ScheduleType::Manual
        ),
    );
    let current_step_count = current_plan.steps.len();
    let Some(model) = planner_model_spec(current_plan.operator_preferences.as_ref()) else {
        let question = planner_llm_unavailable_hint();
        return (
            current_plan.clone(),
            format!("I could not revise the current plan. Clarification needed: {question}"),
            Vec::new(),
            planner_revision_failure_clarifier(question, "planner_model_unavailable"),
            Some(workflow_plan_decomposition_observation(
                &decomposition_profile,
                current_step_count,
            )),
        );
    };

    if !host.is_provider_configured(&model.provider_id).await {
        let question = planner_llm_provider_unconfigured_hint(&model.provider_id);
        return (
            current_plan.clone(),
            format!("I could not revise the current plan. Clarification needed: {question}"),
            Vec::new(),
            planner_revision_failure_clarifier(question, "planner_provider_unconfigured"),
            Some(workflow_plan_decomposition_observation(
                &decomposition_profile,
                current_step_count,
            )),
        );
    }

    let normalization_ctx = PlannerPlanNormalizationContext {
        mode: PlannerPlanMode::Revise,
        plan_id: &current_plan.plan_id,
        planner_version: &current_plan.planner_version,
        plan_source: &current_plan.plan_source,
        original_prompt: &current_plan.original_prompt,
        normalized_prompt: &current_plan.normalized_prompt,
        resolved_workspace_root: &current_plan.workspace_root,
        explicit_schedule: None,
        request_allowed_mcp_servers: &current_plan.allowed_mcp_servers,
        request_operator_preferences: current_plan.operator_preferences.as_ref(),
    };

    match try_llm_revise_workflow_plan(
        host,
        &config,
        &model,
        current_plan,
        conversation,
        message,
        &decomposition_profile,
    )
    .await
    {
        Ok(payload) => parse_llm_revision_payload(
            current_plan,
            payload,
            &normalization_ctx,
            &mut normalize_step,
            &decomposition_profile,
        )
        .unwrap_or_else(|| {
            let question = planner_llm_invalid_response_hint();
            (
                current_plan.clone(),
                format!("I could not revise the current plan. Clarification needed: {question}"),
                Vec::new(),
                planner_revision_failure_clarifier(question, "planner_invalid_response"),
                Some(workflow_plan_decomposition_observation(
                    &decomposition_profile,
                    current_step_count,
                )),
            )
        }),
        Err(failure) => {
            let question = planner_failure_clarifier_hint(&failure);
            (
                current_plan.clone(),
                format!("I could not revise the current plan. Clarification needed: {question}"),
                Vec::new(),
                planner_revision_failure_clarifier(question, "planner_invocation_failed"),
                Some(workflow_plan_decomposition_observation(
                    &decomposition_profile,
                    current_step_count,
                )),
            )
        }
    }
}

async fn try_llm_revise_workflow_plan<M, I, O, H>(
    host: &H,
    config: &PlannerLoopConfig,
    model: &ModelSpec,
    current_plan: &WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    conversation: &WorkflowPlanConversation,
    message: &str,
    decomposition_profile: &crate::decomposition::WorkflowDecompositionProfile,
) -> Result<PlannerRevisionPayload, PlannerInvocationFailure>
where
    M: serde::Serialize,
    I: serde::Serialize,
    O: serde::Serialize,
    H: PlannerLoopHost,
{
    let capability_summary = host
        .capability_summary(&current_plan.allowed_mcp_servers)
        .await;
    let prompt = build_llm_workflow_revision_prompt(
        current_plan,
        conversation,
        message,
        &capability_summary,
        decomposition_profile,
    );

    invoke_planner_json(
        host,
        PlannerLlmInvocation {
            session_title: config.session_title.clone(),
            workspace_root: current_plan.workspace_root.clone(),
            model: model.clone(),
            prompt,
            run_key: format!("workflow-plan-revision:{}", current_plan.plan_id),
            timeout_ms: config.timeout_ms,
            override_env: config.override_env.clone(),
        },
    )
    .await
}

fn parse_llm_revision_payload<M, I, O>(
    current_plan: &WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    payload: PlannerRevisionPayload,
    ctx: &PlannerPlanNormalizationContext<'_, M>,
    normalize_step: &mut impl FnMut(&mut WorkflowPlanStep<I, O>),
    decomposition_profile: &crate::decomposition::WorkflowDecompositionProfile,
) -> Option<(
    WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    String,
    Vec<String>,
    Value,
    Option<Value>,
)>
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone + Default + WorkflowInputRefLike + serde::Serialize + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
{
    match payload.action {
        PlannerRevisionAction::Clarify => {
            let clarifier = payload.clarifier?;
            let question = clarifier.question.trim();
            if question.is_empty() {
                return None;
            }
            let assistant_text = payload
                .assistant_text
                .unwrap_or_else(|| question.to_string());
            Some((
                current_plan.clone(),
                assistant_text,
                Vec::new(),
                json!({
                    "field": clarifier.field.unwrap_or_else(|| "general".to_string()),
                    "question": question,
                    "options": clarifier.options,
                }),
                Some(workflow_plan_decomposition_observation(
                    decomposition_profile,
                    current_plan.steps.len(),
                )),
            ))
        }
        PlannerRevisionAction::Keep => Some((
            current_plan.clone(),
            payload
                .assistant_text
                .unwrap_or_else(|| "I kept the current workflow plan.".to_string()),
            Vec::new(),
            Value::Null,
            Some(workflow_plan_decomposition_observation(
                decomposition_profile,
                current_plan.steps.len(),
            )),
        )),
        PlannerRevisionAction::Revise => {
            let candidate = decode_planner_plan_value(payload.plan?)?;
            let revised_plan =
                normalize_and_validate_planner_plan(candidate, ctx, normalize_step).ok()?;
            let original_step_count = revised_plan.steps.len();
            let (revised_plan, task_budget_report) =
                compact_generated_workflow_plan_to_budget(revised_plan, decomposition_profile);
            if workflow_steps_equal(&revised_plan.steps, &current_plan.steps)
                && revised_plan.title == current_plan.title
                && revised_plan.description == current_plan.description
                && workflow_schedule_equal(&revised_plan.schedule, &current_plan.schedule)
                && revised_plan.workspace_root == current_plan.workspace_root
                && revised_plan.allowed_mcp_servers == current_plan.allowed_mcp_servers
                && revised_plan.operator_preferences == current_plan.operator_preferences
            {
                return Some((
                    current_plan.clone(),
                    payload
                        .assistant_text
                        .unwrap_or_else(|| "I kept the current workflow plan.".to_string()),
                    Vec::new(),
                    Value::Null,
                    Some(workflow_plan_decomposition_observation(
                        decomposition_profile,
                        current_plan.steps.len(),
                    )),
                ));
            }
            let mut change_summary = if payload.change_summary.is_empty() {
                vec!["updated workflow plan".to_string()]
            } else {
                payload.change_summary
            };
            if task_budget_report
                .as_ref()
                .and_then(|report| report.get("status"))
                .and_then(Value::as_str)
                .is_some_and(|status| status == "compacted")
            {
                change_summary.push(format!(
                    "compacted {} generated tasks into {} runnable workflow steps",
                    original_step_count,
                    revised_plan.steps.len()
                ));
            }
            let assistant_text = payload
                .assistant_text
                .unwrap_or_else(|| format!("Updated the plan: {}.", change_summary.join(", ")));
            let observation = workflow_plan_decomposition_observation_with_task_budget(
                decomposition_profile,
                &revised_plan,
                task_budget_report,
            );
            Some((
                revised_plan,
                assistant_text,
                change_summary,
                Value::Null,
                Some(observation),
            ))
        }
    }
}

fn build_llm_workflow_revision_prompt<M, I, O>(
    current_plan: &WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    conversation: &WorkflowPlanConversation,
    message: &str,
    capability_summary: &Value,
    decomposition_profile: &crate::decomposition::WorkflowDecompositionProfile,
) -> String
where
    M: serde::Serialize,
    I: serde::Serialize,
    O: serde::Serialize,
{
    let transcript = conversation
        .messages
        .iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|entry| format!("{}: {}", entry.role, entry.text.trim()))
        .collect::<Vec<_>>()
        .join("\n");

    let common_sections = crate::planner_prompts::workflow_plan_common_sections();
    let decomposition_sections = workflow_plan_decomposition_sections(decomposition_profile);
    let decomposition_observation =
        workflow_plan_decomposition_observation(decomposition_profile, current_plan.steps.len());
    format!(
        concat!(
            "You are revising a Tandem automation workflow plan.\n",
            "Planner intelligence lives in the model. Return JSON only.\n",
            "{}",
            "{}",
            "Current plan decomposition observation:\n{}\n",
            "You may revise title, description, schedule, workspace_root, allowed_mcp_servers, operator_preferences, steps, dependencies, input_refs, and output_contracts.\n",
            "Planner capability summary and runtime MCP inventory (use this instead of inventing tools or hidden capabilities):\n{}\n",
            "Return one of:\n",
            "{{\"action\":\"revise\",\"assistant_text\":\"...\",\"change_summary\":[\"...\"],\"plan\":{{...full WorkflowPlan...}}}}\n",
            "{{\"action\":\"clarify\",\"assistant_text\":\"...\",\"clarifier\":{{\"field\":\"general\",\"question\":\"...\"}}}}\n",
            "{{\"action\":\"keep\",\"assistant_text\":\"...\"}}\n\n",
            "Original prompt:\n{}\n\n",
            "Current plan JSON:\n{}\n\n",
            "Recent planning conversation:\n{}\n\n",
            "User revision request:\n{}\n"
        ),
        common_sections,
        decomposition_sections,
        serde_json::to_string_pretty(&decomposition_observation)
            .unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string_pretty(capability_summary).unwrap_or_else(|_| "{}".to_string()),
        current_plan.original_prompt.trim(),
        serde_json::to_string_pretty(current_plan).unwrap_or_else(|_| "{}".to_string()),
        if transcript.trim().is_empty() {
            "(none yet)".to_string()
        } else {
            transcript
        },
        message.trim(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{McpToolCatalog, PlannerLlmInvoker, PlannerModelRegistry, TelemetrySink};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tandem_workflows::plan_package::{AutomationV2Schedule, AutomationV2ScheduleType};

    type TestPlan = WorkflowPlan<AutomationV2Schedule<Value>, WorkflowPlanStep<Value, Value>>;

    /// Scripted `PlannerLoopHost`: each LLM call pops the next queued
    /// response, and every invocation is captured for assertions.
    struct MockPlannerHost {
        provider_configured: bool,
        responses: Mutex<VecDeque<Result<Value, PlannerInvocationFailure>>>,
        invocations: Mutex<Vec<PlannerLlmInvocation>>,
    }

    impl MockPlannerHost {
        fn scripted(
            responses: impl IntoIterator<Item = Result<Value, PlannerInvocationFailure>>,
        ) -> Self {
            Self {
                provider_configured: true,
                responses: Mutex::new(responses.into_iter().collect()),
                invocations: Mutex::new(Vec::new()),
            }
        }

        fn unconfigured() -> Self {
            Self {
                provider_configured: false,
                responses: Mutex::new(VecDeque::new()),
                invocations: Mutex::new(Vec::new()),
            }
        }

        fn invocation_count(&self) -> usize {
            self.invocations.lock().expect("invocations lock").len()
        }
    }

    #[async_trait::async_trait]
    impl PlannerModelRegistry for MockPlannerHost {
        async fn is_provider_configured(&self, _provider_id: &str) -> bool {
            self.provider_configured
        }
    }

    #[async_trait::async_trait]
    impl McpToolCatalog for MockPlannerHost {
        async fn capability_summary(&self, _allowed_mcp_servers: &[String]) -> Value {
            json!({ "runtime": { "mcp_inventory": [] } })
        }
    }

    #[async_trait::async_trait]
    impl PlannerLlmInvoker for MockPlannerHost {
        async fn invoke_planner_llm(
            &self,
            invocation: PlannerLlmInvocation,
        ) -> Result<Value, PlannerInvocationFailure> {
            self.invocations
                .lock()
                .expect("invocations lock")
                .push(invocation);
            self.responses
                .lock()
                .expect("responses lock")
                .pop_front()
                .expect("test scripted more LLM calls than responses")
        }
    }

    impl TelemetrySink for MockPlannerHost {}

    fn test_step(step_id: &str, depends_on: &[&str]) -> WorkflowPlanStep<Value, Value> {
        WorkflowPlanStep {
            step_id: step_id.to_string(),
            kind: "analysis".to_string(),
            objective: format!("Objective for {step_id}"),
            depends_on: depends_on.iter().map(|dep| dep.to_string()).collect(),
            agent_role: "analyst".to_string(),
            input_refs: Vec::new(),
            output_contract: None,
            metadata: None,
        }
    }

    fn base_plan() -> TestPlan {
        WorkflowPlan {
            plan_id: "wfplan-test".to_string(),
            planner_version: "v1".to_string(),
            plan_source: "unit_test".to_string(),
            original_prompt: "Research the topic and generate a report".to_string(),
            normalized_prompt: "research the topic and generate a report".to_string(),
            confidence: "medium".to_string(),
            title: "Test".to_string(),
            description: None,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: Value::Null,
            },
            execution_target: "automation_v2".to_string(),
            workspace_root: "/tmp/workspace".to_string(),
            steps: vec![test_step("collect_inputs", &[])],
            requires_integrations: vec![],
            allowed_mcp_servers: vec!["github".to_string()],
            // A resolvable planner model lets the loop proceed past the
            // model-availability check to the behavior under test.
            operator_preferences: Some(json!({
                "model_provider": "anthropic",
                "model_id": "test-planner-model",
            })),
            save_options: json!({"can_export_pack": true, "can_save_skill": true}),
        }
    }

    fn empty_conversation() -> WorkflowPlanConversation {
        WorkflowPlanConversation {
            conversation_id: "wfchat-1".to_string(),
            plan_id: "wfplan-test".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            messages: vec![],
        }
    }

    fn test_config() -> PlannerLoopConfig {
        PlannerLoopConfig {
            session_title: "Planner revision".to_string(),
            timeout_ms: 30_000,
            override_env: "TANDEM_TEST_PLANNER".to_string(),
        }
    }

    async fn run_loop(
        host: &MockPlannerHost,
        plan: &TestPlan,
        message: &str,
    ) -> (TestPlan, String, Vec<String>, Value, Option<Value>) {
        revise_workflow_plan_with_planner_loop(
            host,
            plan,
            &empty_conversation(),
            message,
            test_config(),
            |_step| {},
        )
        .await
    }

    fn failure_reason(clarifier: &Value) -> Option<&str> {
        clarifier.get("failure_reason").and_then(Value::as_str)
    }

    fn plan_step_ids(plan: &TestPlan) -> Vec<&str> {
        plan.steps
            .iter()
            .map(|step| step.step_id.as_str())
            .collect()
    }

    #[tokio::test]
    async fn missing_model_preferences_fail_before_any_llm_call() {
        let host = MockPlannerHost::scripted([]);
        let mut plan = base_plan();
        plan.operator_preferences = None;

        let (result, assistant_text, changes, clarifier, observation) =
            run_loop(&host, &plan, "Add a summary step").await;

        assert_eq!(
            failure_reason(&clarifier),
            Some("planner_model_unavailable")
        );
        assert!(clarifier["blocks_activation"].as_bool().unwrap_or(false));
        assert_eq!(plan_step_ids(&result), vec!["collect_inputs"]);
        assert!(assistant_text.contains("could not revise"));
        assert!(changes.is_empty());
        assert!(
            observation.is_some(),
            "decomposition observation always returned"
        );
        assert_eq!(host.invocation_count(), 0);
    }

    #[tokio::test]
    async fn unconfigured_provider_short_circuits_before_llm_invocation() {
        let host = MockPlannerHost::unconfigured();
        let plan = base_plan();

        let (result, _, _, clarifier, _) = run_loop(&host, &plan, "Add a summary step").await;

        assert_eq!(
            failure_reason(&clarifier),
            Some("planner_provider_unconfigured")
        );
        assert_eq!(plan_step_ids(&result), vec!["collect_inputs"]);
        assert_eq!(host.invocation_count(), 0);
    }

    #[tokio::test]
    async fn invoker_failure_keeps_plan_and_reports_invocation_failure() {
        let host = MockPlannerHost::scripted([Err(PlannerInvocationFailure {
            reason: "timeout".to_string(),
            detail: Some("planner session timed out".to_string()),
        })]);
        let plan = base_plan();

        let (result, _, changes, clarifier, _) = run_loop(&host, &plan, "Add a step").await;

        assert_eq!(
            failure_reason(&clarifier),
            Some("planner_invocation_failed")
        );
        assert_eq!(plan_step_ids(&result), vec!["collect_inputs"]);
        assert!(changes.is_empty());
        assert_eq!(host.invocation_count(), 1);
    }

    #[tokio::test]
    async fn malformed_llm_payload_is_an_invocation_failure_not_a_panic() {
        let host = MockPlannerHost::scripted([Ok(json!({ "action": "explode" }))]);
        let plan = base_plan();

        let (result, _, _, clarifier, _) = run_loop(&host, &plan, "Add a step").await;

        assert_eq!(
            failure_reason(&clarifier),
            Some("planner_invocation_failed")
        );
        assert_eq!(plan_step_ids(&result), vec!["collect_inputs"]);
    }

    #[tokio::test]
    async fn revise_without_plan_payload_is_an_invalid_response() {
        let host = MockPlannerHost::scripted([Ok(json!({ "action": "revise" }))]);
        let plan = base_plan();

        let (result, _, _, clarifier, _) = run_loop(&host, &plan, "Add a step").await;

        assert_eq!(failure_reason(&clarifier), Some("planner_invalid_response"));
        assert_eq!(plan_step_ids(&result), vec!["collect_inputs"]);
    }

    #[tokio::test]
    async fn revision_with_unknown_dependency_is_rejected_and_plan_kept() {
        let mut invalid = base_plan();
        invalid
            .steps
            .push(test_step("summarize_inputs", &["ghost_step"]));
        let host = MockPlannerHost::scripted([Ok(json!({
            "action": "revise",
            "plan": serde_json::to_value(&invalid).expect("serialize plan"),
        }))]);
        let plan = base_plan();

        let (result, _, changes, clarifier, _) = run_loop(&host, &plan, "Add a step").await;

        assert_eq!(failure_reason(&clarifier), Some("planner_invalid_response"));
        assert_eq!(plan_step_ids(&result), vec!["collect_inputs"]);
        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn clarify_surfaces_question_field_and_options() {
        let host = MockPlannerHost::scripted([Ok(json!({
            "action": "clarify",
            "clarifier": {
                "field": "schedule",
                "question": "Which timezone should the report use?",
                "options": [
                    { "id": "utc", "label": "UTC" },
                    { "id": "local", "label": "Workspace local time" },
                ],
            },
        }))]);
        let plan = base_plan();

        let (result, assistant_text, changes, clarifier, _) =
            run_loop(&host, &plan, "Schedule it daily").await;

        assert_eq!(clarifier["field"], "schedule");
        assert_eq!(
            clarifier["question"],
            "Which timezone should the report use?"
        );
        assert_eq!(clarifier["options"].as_array().map(Vec::len), Some(2));
        assert!(
            failure_reason(&clarifier).is_none(),
            "clarify is not a failure"
        );
        // With no assistant_text in the payload, the question doubles as text.
        assert_eq!(assistant_text, "Which timezone should the report use?");
        assert_eq!(plan_step_ids(&result), vec!["collect_inputs"]);
        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn clarify_with_blank_question_is_an_invalid_response() {
        let host = MockPlannerHost::scripted([Ok(json!({
            "action": "clarify",
            "clarifier": { "question": "   " },
        }))]);
        let plan = base_plan();

        let (_, _, _, clarifier, _) = run_loop(&host, &plan, "Schedule it daily").await;

        assert_eq!(failure_reason(&clarifier), Some("planner_invalid_response"));
    }

    #[tokio::test]
    async fn keep_returns_current_plan_without_clarifier_or_changes() {
        let host = MockPlannerHost::scripted([Ok(json!({
            "action": "keep",
            "assistant_text": "The plan already covers that.",
        }))]);
        let plan = base_plan();

        let (result, assistant_text, changes, clarifier, observation) =
            run_loop(&host, &plan, "Make sure we collect inputs").await;

        assert_eq!(assistant_text, "The plan already covers that.");
        assert_eq!(clarifier, Value::Null);
        assert!(changes.is_empty());
        assert_eq!(plan_step_ids(&result), vec!["collect_inputs"]);
        assert!(observation.is_some());
    }

    #[tokio::test]
    async fn valid_revision_replaces_plan_and_pins_identity_fields() {
        let mut revised = base_plan();
        revised.title = "Research report with summary".to_string();
        revised
            .steps
            .push(test_step("summarize_inputs", &["collect_inputs"]));
        let mut payload_plan = serde_json::to_value(&revised).expect("serialize plan");
        // The planner must not be able to rewrite plan identity or execution
        // target: normalization pins them back to the current plan's values.
        payload_plan["plan_id"] = json!("evil-override");
        payload_plan["plan_source"] = json!("forged");
        payload_plan["execution_target"] = json!("shell");

        let host = MockPlannerHost::scripted([Ok(json!({
            "action": "revise",
            "assistant_text": "Added a summarize step.",
            "change_summary": ["added summarize_inputs step"],
            "plan": payload_plan,
        }))]);
        let plan = base_plan();

        let (result, assistant_text, changes, clarifier, _) =
            run_loop(&host, &plan, "Add a summary step").await;

        assert_eq!(
            plan_step_ids(&result),
            vec!["collect_inputs", "summarize_inputs"]
        );
        assert_eq!(result.title, "Research report with summary");
        assert_eq!(result.plan_id, "wfplan-test");
        assert_eq!(result.plan_source, "unit_test");
        assert_eq!(result.execution_target, "automation_v2");
        assert_eq!(changes, vec!["added summarize_inputs step".to_string()]);
        assert_eq!(assistant_text, "Added a summarize step.");
        assert_eq!(clarifier, Value::Null);
    }

    #[tokio::test]
    async fn resubmitting_the_same_plan_collapses_to_keep() {
        // Round 1: a real revision, whose output carries the compiler's
        // normalized step metadata.
        let mut revised = base_plan();
        revised
            .steps
            .push(test_step("summarize_inputs", &["collect_inputs"]));
        let host = MockPlannerHost::scripted([Ok(json!({
            "action": "revise",
            "change_summary": ["added summarize_inputs step"],
            "plan": serde_json::to_value(&revised).expect("serialize plan"),
        }))]);
        let plan = base_plan();
        let (after_first, _, first_changes, _, _) =
            run_loop(&host, &plan, "Add a summary step").await;
        assert_eq!(
            first_changes,
            vec!["added summarize_inputs step".to_string()]
        );

        // Round 2: the planner replays the identical plan; the loop must
        // detect the no-op and answer as a keep (no change summary, no
        // clarifier) instead of reporting a phantom revision.
        let host = MockPlannerHost::scripted([Ok(json!({
            "action": "revise",
            "change_summary": ["pretended to change something"],
            "plan": serde_json::to_value(&after_first).expect("serialize plan"),
        }))]);

        let (after_second, assistant_text, changes, clarifier, _) =
            run_loop(&host, &after_first, "Improve it").await;

        assert_eq!(assistant_text, "I kept the current workflow plan.");
        assert!(changes.is_empty());
        assert_eq!(clarifier, Value::Null);
        assert_eq!(plan_step_ids(&after_second), plan_step_ids(&after_first));
    }

    #[tokio::test]
    async fn invoker_receives_run_key_prompt_and_config_passthrough() {
        let host = MockPlannerHost::scripted([Ok(json!({ "action": "keep" }))]);
        let plan = base_plan();

        let _ = run_loop(&host, &plan, "Tighten the report scope").await;

        let invocations = host.invocations.lock().expect("invocations lock");
        assert_eq!(invocations.len(), 1);
        let invocation = &invocations[0];
        assert_eq!(invocation.run_key, "workflow-plan-revision:wfplan-test");
        assert_eq!(invocation.session_title, "Planner revision");
        assert_eq!(invocation.timeout_ms, 30_000);
        assert_eq!(invocation.override_env, "TANDEM_TEST_PLANNER");
        assert_eq!(invocation.workspace_root, "/tmp/workspace");
        assert!(invocation.prompt.contains("User revision request"));
        assert!(invocation.prompt.contains("Tighten the report scope"));
        assert!(invocation.prompt.contains("Current plan JSON"));
    }

    #[test]
    fn revise_prompt_surfaces_decomposition_guidance() {
        let current_plan: WorkflowPlan<
            AutomationV2Schedule<Value>,
            WorkflowPlanStep<Value, Value>,
        > = WorkflowPlan {
            plan_id: "wfplan-test".to_string(),
            planner_version: "v1".to_string(),
            plan_source: "unit_test".to_string(),
            original_prompt: "Research the topic and generate a report".to_string(),
            normalized_prompt: "research the topic and generate a report".to_string(),
            confidence: "medium".to_string(),
            title: "Test".to_string(),
            description: None,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: Value::Null,
            },
            execution_target: "automation_v2".to_string(),
            workspace_root: "/tmp/workspace".to_string(),
            steps: vec![],
            requires_integrations: vec![],
            allowed_mcp_servers: vec!["github".to_string()],
            operator_preferences: None,
            save_options: json!({"can_export_pack": true, "can_save_skill": true}),
        };
        let conversation: WorkflowPlanConversation = WorkflowPlanConversation {
            conversation_id: "wfchat-1".to_string(),
            plan_id: "wfplan-test".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            messages: vec![],
        };
        let profile = derive_workflow_decomposition_profile(
            &current_plan.original_prompt,
            &current_plan.allowed_mcp_servers,
            &infer_explicit_output_targets(&current_plan.original_prompt),
            false,
        );
        let prompt = build_llm_workflow_revision_prompt(
            &current_plan,
            &conversation,
            "Add more microtasks.",
            &json!({"runtime": {"mcp_inventory": []}}),
            &profile,
        );

        assert!(prompt.contains("Decomposition profile:"));
        assert!(prompt.contains("within 8 leaf tasks"));
        assert!(prompt.contains("one primary objective"));
    }
}
