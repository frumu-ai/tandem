use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_core::permission_defaults::build_mode_permission_rules;
use tandem_core::tool_router::{
    classify_intent, select_tool_subset, should_escalate_auto_tools, ToolIntent,
};
use tandem_providers::Provider;
use tandem_server::app::state::AppState;
use tandem_server::{
    AutomationV2Schedule, AutomationV2ScheduleType, AutomationV2Spec, AutomationV2Status,
    RoutineMisfirePolicy,
};
use tandem_tools::ToolDispatchSource;
use tandem_types::{
    AuthorityChain, HumanActor, RequestPrincipal, Session, TenantContext, VerifiedTenantContext,
};
use tokio::sync::broadcast;
use tower::ServiceExt;

use crate::dataset::{EvalDataset, EvalTestCase};
use crate::scripted_provider::{
    ScriptedEvalProvider, ScriptedResponse, SCRIPTED_MODEL_ID, SCRIPTED_PROVIDER_ID,
};
use crate::{bootstrap_eval_app_state, test_case_to_spec, EvalBootstrapOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticAuthoringAcceptanceThresholds {
    pub required_pass_rate: f64,
    #[serde(default)]
    pub required_coverage: Vec<String>,
    pub execution_profile: String,
    #[serde(default)]
    pub live_provider_calls: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct AgenticAuthoringDataset {
    #[serde(flatten)]
    dataset: EvalDataset,
    acceptance: AgenticAuthoringAcceptanceThresholds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticAuthoringCaseResult {
    pub test_id: String,
    pub description: String,
    pub scenario: String,
    pub passed: bool,
    pub tags: Vec<String>,
    pub evidence: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticAuthoringAcceptanceReport {
    pub dataset_name: String,
    pub dataset_version: String,
    pub execution_profile: String,
    pub live_provider_calls: bool,
    pub required_pass_rate: f64,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub pass_rate: f64,
    pub required_coverage: Vec<String>,
    pub observed_coverage: Vec<String>,
    pub missing_coverage: Vec<String>,
    pub gate_passed: bool,
    pub test_results: Vec<AgenticAuthoringCaseResult>,
}

impl AgenticAuthoringAcceptanceReport {
    pub fn summary(&self) -> String {
        let status = if self.gate_passed { "PASS" } else { "FAIL" };
        let mut summary = format!(
            "=== Agentic Product Authoring Acceptance: {status} ===\nDataset: {} v{}\nProfile: {} (live provider calls: {})\nTests: {}/{} passed ({:.1}%; required {:.1}%)\nCoverage: {}/{} required categories\n",
            self.dataset_name,
            self.dataset_version,
            self.execution_profile,
            self.live_provider_calls,
            self.passed_tests,
            self.total_tests,
            self.pass_rate * 100.0,
            self.required_pass_rate * 100.0,
            self.required_coverage.len() - self.missing_coverage.len(),
            self.required_coverage.len(),
        );
        if !self.missing_coverage.is_empty() {
            summary.push_str(&format!(
                "Missing coverage: {}\n",
                self.missing_coverage.join(", ")
            ));
        }
        for result in &self.test_results {
            let marker = if result.passed { "[OK]" } else { "[FAIL]" };
            summary.push_str(&format!(
                "{marker} {} ({}): {}\n",
                result.test_id,
                result.scenario,
                result.error.as_deref().unwrap_or(&result.description)
            ));
        }
        summary
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AgenticAuthoringScenario {
    Route {
        prompt: String,
        expected_intent: String,
        #[serde(default)]
        required_tools: Vec<String>,
        #[serde(default)]
        forbidden_tools: Vec<String>,
        #[serde(default)]
        allowlist: Vec<String>,
        #[serde(default)]
        model_must_run: bool,
    },
    ChatModelExecution {
        prompt: String,
        response_marker: String,
        #[serde(default)]
        required_prompt_fragments: Vec<String>,
        #[serde(default)]
        forbidden_response_fragments: Vec<String>,
    },
    ToolContract {
        tools: Vec<ToolContractExpectation>,
        permissions: Vec<PermissionExpectation>,
    },
    IdentityBoundary {
        tenant_id: String,
        actor_id: String,
        #[serde(default)]
        forbidden_capability_fields: Vec<String>,
    },
    DraftLifecycle {
        tenant_id: String,
        actor_id: String,
        idempotency_key: String,
        expected_status: String,
        schedule: ScheduleExpectation,
        max_parallel_agents: u32,
        #[serde(default)]
        dependencies: HashMap<String, Vec<String>>,
        #[serde(default)]
        parallel_node_ids: Vec<String>,
        assistant_claim: String,
    },
    ActiveArtifact {
        tenant_id: String,
        foreign_tenant_id: String,
        initial_active_id: String,
        initial_active_revision: u32,
        selected_id: String,
        selected_revision: u32,
        foreign_id: String,
        prompt_injection: String,
    },
    ConfirmationBoundary {
        tenant_id: String,
        actor_id: String,
        automation_id: String,
        attempted_action: String,
        expected_error_fragment: String,
        #[serde(default)]
        permission_prompts: Vec<String>,
    },
    FailureTaxonomy {
        failures: Vec<FailureExpectation>,
    },
}

impl AgenticAuthoringScenario {
    fn name(&self) -> &'static str {
        match self {
            Self::Route { .. } => "route",
            Self::ChatModelExecution { .. } => "chat_model_execution",
            Self::ToolContract { .. } => "tool_contract",
            Self::IdentityBoundary { .. } => "identity_boundary",
            Self::DraftLifecycle { .. } => "draft_lifecycle",
            Self::ActiveArtifact { .. } => "active_artifact",
            Self::ConfirmationBoundary { .. } => "confirmation_boundary",
            Self::FailureTaxonomy { .. } => "failure_taxonomy",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ToolContractExpectation {
    name: String,
    risk_tier: String,
    #[serde(default)]
    required_arguments: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PermissionExpectation {
    tool: String,
    action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScheduleExpectation {
    #[serde(rename = "type")]
    schedule_type: String,
    #[serde(default)]
    cron_expression: Option<String>,
    timezone: String,
}

#[derive(Debug, Clone, Deserialize)]
struct FailureExpectation {
    status: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    expected_category: Option<String>,
}

pub async fn run_agentic_product_authoring_acceptance(
    dataset_path: &Path,
) -> anyhow::Result<AgenticAuthoringAcceptanceReport> {
    let document = load_agentic_dataset(dataset_path)?;
    if document.acceptance.live_provider_calls {
        bail!("agentic authoring CI profile must not enable live provider calls");
    }
    let state = bootstrap_eval_app_state(EvalBootstrapOptions {
        spawn_executor: false,
        ..EvalBootstrapOptions::default()
    })
    .await?;

    let mut results = Vec::new();
    for test_case in document.dataset.sorted_by_priority() {
        if !test_case.enabled {
            continue;
        }
        let scenario = scenario_for_case(test_case)?;
        let scenario_name = scenario.name().to_string();
        let evaluation = evaluate_case(&state, test_case, &scenario).await;
        let (passed, evidence, error) = match evaluation {
            Ok(evidence) => (true, evidence, None),
            Err(error) => (false, Value::Null, Some(error.to_string())),
        };
        results.push(AgenticAuthoringCaseResult {
            test_id: test_case.id.clone(),
            description: test_case.description.clone(),
            scenario: scenario_name,
            passed,
            tags: test_case.tags.clone(),
            evidence,
            error,
        });
    }

    Ok(build_report(
        &document.dataset,
        &document.acceptance,
        results,
    ))
}

fn load_agentic_dataset(path: &Path) -> anyhow::Result<AgenticAuthoringDataset> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
}

fn scenario_for_case(test_case: &EvalTestCase) -> anyhow::Result<AgenticAuthoringScenario> {
    let value = test_case
        .automation_spec
        .config
        .get("agentic_acceptance")
        .cloned()
        .with_context(|| {
            format!(
                "{} is missing automation_spec.config.agentic_acceptance",
                test_case.id
            )
        })?;
    serde_json::from_value(value)
        .with_context(|| format!("{} has an invalid acceptance fixture", test_case.id))
}

fn build_report(
    dataset: &EvalDataset,
    thresholds: &AgenticAuthoringAcceptanceThresholds,
    test_results: Vec<AgenticAuthoringCaseResult>,
) -> AgenticAuthoringAcceptanceReport {
    let total_tests = test_results.len();
    let passed_tests = test_results.iter().filter(|result| result.passed).count();
    let failed_tests = total_tests.saturating_sub(passed_tests);
    let pass_rate = if total_tests == 0 {
        0.0
    } else {
        passed_tests as f64 / total_tests as f64
    };
    let observed = test_results
        .iter()
        .filter(|result| result.passed)
        .flat_map(|result| result.tags.iter().cloned())
        .collect::<BTreeSet<_>>();
    let missing = thresholds
        .required_coverage
        .iter()
        .filter(|required| !observed.contains(*required))
        .cloned()
        .collect::<Vec<_>>();
    let gate_passed = failed_tests == 0
        && pass_rate + f64::EPSILON >= thresholds.required_pass_rate
        && missing.is_empty()
        && !thresholds.live_provider_calls;

    AgenticAuthoringAcceptanceReport {
        dataset_name: dataset.name.clone(),
        dataset_version: dataset.version.clone(),
        execution_profile: thresholds.execution_profile.clone(),
        live_provider_calls: thresholds.live_provider_calls,
        required_pass_rate: thresholds.required_pass_rate,
        total_tests,
        passed_tests,
        failed_tests,
        pass_rate,
        required_coverage: thresholds.required_coverage.clone(),
        observed_coverage: observed.into_iter().collect(),
        missing_coverage: missing,
        gate_passed,
        test_results,
    }
}

async fn evaluate_case(
    state: &AppState,
    test_case: &EvalTestCase,
    scenario: &AgenticAuthoringScenario,
) -> anyhow::Result<Value> {
    match scenario {
        AgenticAuthoringScenario::Route {
            prompt,
            expected_intent,
            required_tools,
            forbidden_tools,
            allowlist,
            model_must_run,
        } => {
            evaluate_route(
                state,
                prompt,
                expected_intent,
                required_tools,
                forbidden_tools,
                allowlist,
                *model_must_run,
            )
            .await
        }
        AgenticAuthoringScenario::ChatModelExecution {
            prompt,
            response_marker,
            required_prompt_fragments,
            forbidden_response_fragments,
        } => {
            evaluate_chat_model_execution(
                state,
                prompt,
                response_marker,
                required_prompt_fragments,
                forbidden_response_fragments,
            )
            .await
        }
        AgenticAuthoringScenario::ToolContract { tools, permissions } => {
            evaluate_tool_contract(state, tools, permissions).await
        }
        AgenticAuthoringScenario::IdentityBoundary {
            tenant_id,
            actor_id,
            forbidden_capability_fields,
        } => {
            evaluate_identity_boundary(state, tenant_id, actor_id, forbidden_capability_fields)
                .await
        }
        AgenticAuthoringScenario::DraftLifecycle {
            tenant_id,
            actor_id,
            idempotency_key,
            expected_status,
            schedule,
            max_parallel_agents,
            dependencies,
            parallel_node_ids,
            assistant_claim,
        } => {
            evaluate_draft_lifecycle(
                state,
                test_case,
                tenant_id,
                actor_id,
                idempotency_key,
                expected_status,
                schedule,
                *max_parallel_agents,
                dependencies,
                parallel_node_ids,
                assistant_claim,
            )
            .await
        }
        AgenticAuthoringScenario::ActiveArtifact {
            tenant_id,
            foreign_tenant_id,
            initial_active_id,
            initial_active_revision,
            selected_id,
            selected_revision,
            foreign_id,
            prompt_injection,
        } => {
            evaluate_active_artifact(
                state,
                tenant_id,
                foreign_tenant_id,
                initial_active_id,
                *initial_active_revision,
                selected_id,
                *selected_revision,
                foreign_id,
                prompt_injection,
            )
            .await
        }
        AgenticAuthoringScenario::ConfirmationBoundary {
            tenant_id,
            actor_id,
            automation_id,
            attempted_action,
            expected_error_fragment,
            permission_prompts,
        } => {
            evaluate_confirmation_boundary(
                state,
                test_case,
                tenant_id,
                actor_id,
                automation_id,
                attempted_action,
                expected_error_fragment,
                permission_prompts,
            )
            .await
        }
        AgenticAuthoringScenario::FailureTaxonomy { failures } => {
            evaluate_failure_taxonomy(failures)
        }
    }
}

async fn evaluate_route(
    state: &AppState,
    prompt: &str,
    expected_intent: &str,
    required_tools: &[String],
    forbidden_tools: &[String],
    allowlist: &[String],
    model_must_run: bool,
) -> anyhow::Result<Value> {
    let intent = classify_intent(prompt);
    let actual_intent = intent_name(intent);
    if actual_intent != expected_intent {
        bail!("expected intent {expected_intent}, got {actual_intent}");
    }
    if model_must_run && !should_escalate_auto_tools(intent, prompt, "") {
        bail!("authoring prompt was not marked for model/tool execution");
    }
    let allowlist = allowlist.iter().cloned().collect::<HashSet<_>>();
    let selected = select_tool_subset(state.tools.list().await, intent, &allowlist, false);
    let selected_names = selected
        .iter()
        .map(|schema| schema.name.clone())
        .collect::<BTreeSet<_>>();
    for required in required_tools {
        if !selected_names.contains(required) {
            bail!("required tool {required} was not selected");
        }
    }
    for forbidden in forbidden_tools {
        if selected_names.contains(forbidden) {
            bail!("forbidden tool {forbidden} was selected");
        }
    }
    Ok(json!({
        "prompt": prompt,
        "intent": actual_intent,
        "selected_tools": selected_names,
        "model_execution_required": model_must_run,
    }))
}

async fn evaluate_chat_model_execution(
    state: &AppState,
    prompt: &str,
    response_marker: &str,
    required_prompt_fragments: &[String],
    forbidden_response_fragments: &[String],
) -> anyhow::Result<Value> {
    let provider = Arc::new(ScriptedEvalProvider::new().with_pattern(
        prompt,
        ScriptedResponse::Text(format!(
            "{response_marker}: request reached the model; no product mutation is claimed."
        )),
    ));
    state
        .providers
        .replace_for_test(
            vec![provider.clone() as Arc<dyn Provider>],
            Some(SCRIPTED_PROVIDER_ID.to_string()),
        )
        .await;

    let session = Session::new(
        Some("Agentic authoring model execution eval".to_string()),
        Some(std::env::current_dir()?.to_string_lossy().to_string()),
    );
    let session_id = session.id.clone();
    state.storage.save_session(session).await?;
    let app = tandem_server::http::build_router_with_extensions(state.clone(), &[]);
    let request = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_sync"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{ "type": "text", "text": prompt }],
                "model": {
                    "provider_id": SCRIPTED_PROVIDER_ID,
                    "model_id": SCRIPTED_MODEL_ID,
                },
                "tool_mode": "auto",
            })
            .to_string(),
        ))?;
    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    if status != StatusCode::OK {
        bail!(
            "prompt_sync returned {status}: {}",
            String::from_utf8_lossy(&body)
        );
    }
    let messages: Vec<Value> = serde_json::from_slice(&body)?;
    let assistant = latest_assistant_text(&messages);
    if !assistant.contains(response_marker) {
        bail!("valid authoring prompt did not reach the scripted model");
    }
    for forbidden in forbidden_response_fragments {
        if assistant
            .to_ascii_lowercase()
            .contains(&forbidden.to_ascii_lowercase())
        {
            bail!("assistant response contains forbidden fragment {forbidden:?}");
        }
    }
    let captured_prompts = provider.call_log().await;
    if captured_prompts.is_empty() {
        bail!("scripted provider recorded no model call");
    }
    let captured = captured_prompts.join("\n");
    for fragment in required_prompt_fragments {
        if !captured.contains(fragment) {
            bail!("model prompt is missing required operator contract fragment {fragment:?}");
        }
    }
    Ok(json!({
        "session_id": session_id,
        "provider": SCRIPTED_PROVIDER_ID,
        "provider_call_count": captured_prompts.len(),
        "response_marker": response_marker,
        "operator_contract_fragments": required_prompt_fragments,
        "intercepted": false,
    }))
}

async fn evaluate_tool_contract(
    state: &AppState,
    expectations: &[ToolContractExpectation],
    permission_expectations: &[PermissionExpectation],
) -> anyhow::Result<Value> {
    let schemas = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| (schema.name.clone(), schema))
        .collect::<HashMap<_, _>>();
    let mut tool_evidence = Vec::new();
    for expected in expectations {
        let schema = schemas
            .get(&expected.name)
            .with_context(|| format!("missing first-party tool {}", expected.name))?;
        let risk = schema
            .security
            .risk_tier
            .as_ref()
            .map(|risk| risk.as_str())
            .unwrap_or("unspecified");
        if risk != expected.risk_tier {
            bail!(
                "tool {} expected risk {}, got {}",
                expected.name,
                expected.risk_tier,
                risk
            );
        }
        let required = schema
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>();
        for argument in &expected.required_arguments {
            if !required.contains(argument.as_str()) {
                bail!(
                    "tool {} does not require argument {argument}",
                    expected.name
                );
            }
        }
        tool_evidence.push(json!({
            "name": expected.name,
            "risk_tier": risk,
            "required_arguments": required,
        }));
    }

    let rules = build_mode_permission_rules(None);
    let mut permission_evidence = Vec::new();
    for expected in permission_expectations {
        let matched = rules
            .iter()
            .any(|rule| rule.permission == expected.tool && rule.action == expected.action);
        if !matched {
            bail!(
                "tool {} expected default permission action {}",
                expected.tool,
                expected.action
            );
        }
        permission_evidence.push(json!({
            "tool": expected.tool,
            "action": expected.action,
        }));
    }
    Ok(json!({
        "tools": tool_evidence,
        "permissions": permission_evidence,
    }))
}

async fn evaluate_identity_boundary(
    state: &AppState,
    tenant_id: &str,
    actor_id: &str,
    forbidden_capability_fields: &[String],
) -> anyhow::Result<Value> {
    let tenant = eval_tenant(tenant_id, actor_id);
    let verified = verified_context(tenant.clone(), actor_id, Vec::new(), Vec::new());
    let session = save_chat_session(state, tenant.clone(), verified.clone(), "identity").await?;
    let foreign = verified_context(
        eval_tenant("foreign-identity", "attacker"),
        "attacker",
        vec!["owner".to_string()],
        vec!["automation.control".to_string()],
    );
    let context = dispatch_context(
        state,
        &tenant,
        &verified,
        &session.id,
        "workflow_plan_capabilities",
        "identity-boundary",
    );
    let result = state
        .tool_dispatcher
        .dispatch(
            "workflow_plan_capabilities",
            json!({
                "chat_session_id": session.id,
                "__dispatch_session_id": "model-supplied-session",
                "__verified_tenant_context": foreign,
            }),
            context,
        )
        .await?;
    if result.metadata.get("secrets_included") != Some(&Value::Bool(false)) {
        bail!("capability inspection must explicitly omit secrets");
    }
    if result.metadata.pointer("/tenant/org_id") != Some(&json!(tenant_id)) {
        bail!("capability inspection did not use the trusted dispatch tenant");
    }
    let serialized = result.metadata.to_string().to_ascii_lowercase();
    for field in forbidden_capability_fields {
        if serialized.contains(&field.to_ascii_lowercase()) {
            bail!("capability inspection exposed forbidden credential field {field}");
        }
    }
    Ok(json!({
        "trusted_tenant": tenant_id,
        "trusted_actor": actor_id,
        "model_identity_override_ignored": true,
        "secrets_included": false,
        "external_connections_reported_without_secret_values": true,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn evaluate_draft_lifecycle(
    state: &AppState,
    test_case: &EvalTestCase,
    tenant_id: &str,
    actor_id: &str,
    idempotency_key: &str,
    expected_status: &str,
    schedule: &ScheduleExpectation,
    max_parallel_agents: u32,
    dependencies: &HashMap<String, Vec<String>>,
    parallel_node_ids: &[String],
    assistant_claim: &str,
) -> anyhow::Result<Value> {
    let tenant = eval_tenant(tenant_id, actor_id);
    let verified = verified_context(tenant.clone(), actor_id, Vec::new(), Vec::new());
    let session = save_chat_session(state, tenant.clone(), verified.clone(), "draft").await?;
    let mut automation = test_case_to_spec(test_case);
    automation.execution.max_parallel_agents = Some(max_parallel_agents);
    automation.schedule = schedule_from_expectation(schedule)?;
    for node in &mut automation.flow.nodes {
        if let Some(depends_on) = dependencies.get(&node.node_id) {
            node.depends_on = depends_on.clone();
        }
    }
    let planner_session_id = format!("planner-{}", test_case.id);
    let plan_id = format!("plan-{}", test_case.id);
    let workspace_root = std::env::temp_dir()
        .join("tandem-agentic-authoring-eval")
        .to_string_lossy()
        .to_string();
    let steps = automation
        .flow
        .nodes
        .iter()
        .map(|node| {
            json!({
                "step_id": node.node_id,
                "kind": "task",
                "objective": node.objective,
                "depends_on": node.depends_on,
                "agent_role": node.agent_id,
                "input_refs": node.input_refs,
                "output_contract": node.output_contract,
            })
        })
        .collect::<Vec<_>>();
    let plan = json!({
        "plan_id": plan_id,
        "planner_version": "agentic-authoring-eval-v1",
        "plan_source": "recorded_eval_fixture",
        "original_prompt": test_case.description,
        "normalized_prompt": test_case.description,
        "confidence": "recorded",
        "title": automation.name,
        "description": automation.description,
        "schedule": automation.schedule,
        "execution_target": "automation_v2",
        "workspace_root": workspace_root,
        "steps": steps,
        "requires_integrations": [],
        "allowed_mcp_servers": [],
        "operator_preferences": {
            "execution_mode": "team",
            "max_parallel_agents": max_parallel_agents,
        },
        "save_options": { "materialize_as_draft": true }
    });
    tandem_server::eval_support::put_product_authoring_planner_session_fixture(
        state,
        planner_session_fixture_with_plan(&tenant, &session.id, &planner_session_id, 1, 1, plan),
    )
    .await?;
    let args = json!({
        "chat_session_id": session.id,
        "planner_session_id": planner_session_id,
        "expected_revision": 1,
        "idempotency_key": idempotency_key,
        "__verified_tenant_context": verified_context(
            eval_tenant("foreign-draft", "attacker"),
            "attacker",
            vec!["owner".to_string()],
            Vec::new(),
        ),
    });
    let mut events = state.event_bus.subscribe();
    let first = state
        .tool_dispatcher
        .dispatch(
            "workflow_plan_materialize",
            args.clone(),
            dispatch_context(
                state,
                &tenant,
                &verified,
                &session.id,
                "workflow_plan_materialize",
                "draft-materialize",
            ),
        )
        .await?;
    let dispatch_event = next_dispatch_event(&mut events, "workflow_plan_materialize").await?;
    let materialized_id = first
        .metadata
        .pointer("/resource/id")
        .and_then(Value::as_str)
        .context("materialization returned no automation id")?
        .to_string();
    let stored = state
        .get_automation_v2(&materialized_id)
        .await
        .context("tool claimed success without a persisted automation")?;
    validate_authoritative_claim(assistant_claim, &first.metadata, Some(&stored))?;
    if automation_status_name(&stored.status) != expected_status {
        bail!(
            "draft status expected {expected_status}, got {}",
            automation_status_name(&stored.status)
        );
    }
    if stored.creator_id != actor_id {
        bail!("persisted audit actor did not come from trusted chat identity");
    }
    if stored.tenant_context().org_id != tenant_id {
        bail!("persisted automation escaped the authenticated tenant");
    }
    if stored.execution.max_parallel_agents != Some(max_parallel_agents) {
        bail!("persisted automation lost its parallel execution limit");
    }
    assert_schedule(&stored, schedule)?;
    assert_parallel_group(&stored, parallel_node_ids)?;

    let replay = state
        .tool_dispatcher
        .dispatch(
            "workflow_plan_materialize",
            args.clone(),
            dispatch_context(
                state,
                &tenant,
                &verified,
                &session.id,
                "workflow_plan_materialize",
                "materialize-replay",
            ),
        )
        .await?;
    if replay.metadata.pointer("/resource/id") != Some(&json!(materialized_id)) {
        bail!("idempotent replay did not return the original resource");
    }
    let mut conflicting = args;
    conflicting["overlap_decision"] = json!("new");
    let conflict = state
        .tool_dispatcher
        .dispatch(
            "workflow_plan_materialize",
            conflicting,
            dispatch_context(
                state,
                &tenant,
                &verified,
                &session.id,
                "workflow_plan_materialize",
                "materialize-conflict",
            ),
        )
        .await
        .expect_err("same idempotency key with a different request must fail");
    if !conflict
        .to_string()
        .contains("different workflow apply request")
    {
        bail!("idempotency conflict returned an unexpected error: {conflict}");
    }
    let persisted_count = state
        .list_automations_v2()
        .await
        .into_iter()
        .filter(|row| row.automation_id == materialized_id)
        .count();
    if persisted_count != 1 {
        bail!("idempotent retries persisted {persisted_count} matching automations");
    }
    assert_dispatch_identity(&dispatch_event, &session.id, tenant_id, "succeeded")?;
    Ok(json!({
        "resource_id": materialized_id,
        "status": expected_status,
        "trusted_actor": actor_id,
        "schedule": schedule,
        "max_parallel_agents": max_parallel_agents,
        "parallel_node_ids": parallel_node_ids,
        "idempotent_replay": true,
        "conflicting_retry_rejected": true,
        "persisted_count": persisted_count,
        "assistant_claim": assistant_claim,
        "authoritative_outcome_verified": true,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn evaluate_active_artifact(
    state: &AppState,
    tenant_id: &str,
    foreign_tenant_id: &str,
    initial_active_id: &str,
    initial_active_revision: u32,
    selected_id: &str,
    selected_revision: u32,
    foreign_id: &str,
    prompt_injection: &str,
) -> anyhow::Result<Value> {
    let actor_id = format!("{tenant_id}-author");
    let tenant = eval_tenant(tenant_id, &actor_id);
    let verified = verified_context(tenant.clone(), &actor_id, Vec::new(), Vec::new());
    let session = save_chat_session(state, tenant.clone(), verified.clone(), "artifact").await?;
    tandem_server::eval_support::put_product_authoring_planner_session_fixture(
        state,
        planner_session_fixture(
            &tenant,
            &session.id,
            initial_active_id,
            initial_active_revision,
            20,
        ),
    )
    .await?;
    tandem_server::eval_support::put_product_authoring_planner_session_fixture(
        state,
        planner_session_fixture(&tenant, &session.id, selected_id, selected_revision, 10),
    )
    .await?;
    let foreign_tenant = eval_tenant(foreign_tenant_id, "foreign-author");
    tandem_server::eval_support::put_product_authoring_planner_session_fixture(
        state,
        planner_session_fixture(&foreign_tenant, &session.id, foreign_id, 99, 30),
    )
    .await?;

    let initial = tandem_server::eval_support::product_authoring_artifact_context(
        state,
        &tenant,
        &session.id,
    )
    .await;
    assert_active_artifact(
        &initial,
        initial_active_id,
        initial_active_revision,
        foreign_id,
    )?;

    let read = state
        .tool_dispatcher
        .dispatch(
            "workflow_plan_read",
            json!({
                "chat_session_id": session.id,
                "planner_session_id": selected_id,
            }),
            dispatch_context(
                state,
                &tenant,
                &verified,
                &session.id,
                "workflow_plan_read",
                "artifact-select",
            ),
        )
        .await?;
    if read
        .metadata
        .pointer("/planner_session/draft/plan_revision")
        != Some(&json!(selected_revision))
    {
        bail!("explicit artifact read lost revision continuity");
    }
    let selected = tandem_server::eval_support::product_authoring_artifact_context(
        state,
        &tenant,
        &session.id,
    )
    .await;
    assert_active_artifact(&selected, selected_id, selected_revision, foreign_id)?;
    if classify_intent(prompt_injection) != ToolIntent::ProductAuthoring {
        bail!("prompt-injection authoring request escaped product routing");
    }
    Ok(json!({
        "initial_active": initial_active_id,
        "initial_revision": initial_active_revision,
        "selected_active": selected_id,
        "selected_revision": selected_revision,
        "foreign_artifact_hidden": foreign_id,
        "prompt_injection_routed_to_governed_authoring": true,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn evaluate_confirmation_boundary(
    state: &AppState,
    test_case: &EvalTestCase,
    tenant_id: &str,
    actor_id: &str,
    automation_id: &str,
    attempted_action: &str,
    expected_error_fragment: &str,
    permission_prompts: &[String],
) -> anyhow::Result<Value> {
    let tenant = eval_tenant(tenant_id, actor_id);
    let verified = verified_context(tenant.clone(), actor_id, Vec::new(), Vec::new());
    let session =
        save_chat_session(state, tenant.clone(), verified.clone(), "confirmation").await?;
    let mut automation = test_case_to_spec(test_case);
    automation.automation_id = automation_id.to_string();
    automation.status = AutomationV2Status::Active;
    automation.creator_id = actor_id.to_string();
    automation.set_tenant_context(&tenant);
    state.put_automation_v2(automation).await?;

    let rules = build_mode_permission_rules(None);
    for tool in permission_prompts {
        if !rules
            .iter()
            .any(|rule| rule.permission == *tool && rule.action == "ask")
        {
            bail!("consequential tool {tool} does not require confirmation");
        }
    }
    let mut events = state.event_bus.subscribe();
    let error = state
        .tool_dispatcher
        .dispatch(
            "automation_control",
            json!({
                "chat_session_id": session.id,
                "automation_id": automation_id,
                "action": attempted_action,
                "idempotency_key": format!("confirmation-{automation_id}"),
                "reason": "deterministic authorization probe",
            }),
            dispatch_context(
                state,
                &tenant,
                &verified,
                &session.id,
                "automation_control",
                "confirmation-denied",
            ),
        )
        .await
        .expect_err("unprivileged product control must fail");
    if !error.to_string().contains(expected_error_fragment) {
        bail!("permission denial returned an unexpected error: {error}");
    }
    let event = next_dispatch_event(&mut events, "automation_control").await?;
    assert_dispatch_identity(&event, &session.id, tenant_id, "failed")?;
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .context("denied control removed the automation")?;
    if stored.status != AutomationV2Status::Active {
        bail!("denied control mutated automation status");
    }
    Ok(json!({
        "automation_id": automation_id,
        "attempted_action": attempted_action,
        "permission_default": "ask",
        "permission_denied": true,
        "status_after_denial": automation_status_name(&stored.status),
        "unauthorized_side_effects": 0,
        "audit_actor_session": session.id,
    }))
}

fn evaluate_failure_taxonomy(failures: &[FailureExpectation]) -> anyhow::Result<Value> {
    let mut evidence = Vec::new();
    for failure in failures {
        let actual = tandem_server::eval_support::product_authoring_failure_category(
            &failure.status,
            failure.error.as_deref(),
        )
        .map(str::to_string);
        if actual != failure.expected_category {
            bail!(
                "failure ({}, {:?}) expected {:?}, got {:?}",
                failure.status,
                failure.error,
                failure.expected_category,
                actual
            );
        }
        evidence.push(json!({
            "status": failure.status,
            "error": failure.error,
            "category": actual,
        }));
    }
    Ok(json!({
        "failures": evidence,
        "stable_categories": true,
        "cancellation_distinct_from_timeout": true,
        "missing_connection_distinct_from_internal_identity": true,
    }))
}

fn schedule_from_expectation(
    expected: &ScheduleExpectation,
) -> anyhow::Result<AutomationV2Schedule> {
    let schedule_type = match expected.schedule_type.as_str() {
        "cron" => AutomationV2ScheduleType::Cron,
        "interval" => AutomationV2ScheduleType::Interval,
        "manual" => AutomationV2ScheduleType::Manual,
        other => bail!("unsupported eval schedule type {other}"),
    };
    Ok(AutomationV2Schedule {
        schedule_type,
        cron_expression: expected.cron_expression.clone(),
        interval_seconds: None,
        timezone: expected.timezone.clone(),
        misfire_policy: RoutineMisfirePolicy::RunOnce,
    })
}

fn assert_schedule(
    automation: &AutomationV2Spec,
    expected: &ScheduleExpectation,
) -> anyhow::Result<()> {
    let actual = match automation.schedule.schedule_type {
        AutomationV2ScheduleType::Cron => "cron",
        AutomationV2ScheduleType::Interval => "interval",
        AutomationV2ScheduleType::Manual => "manual",
    };
    if actual != expected.schedule_type
        || automation.schedule.cron_expression != expected.cron_expression
        || automation.schedule.timezone != expected.timezone
    {
        bail!("persisted schedule does not match the authored schedule");
    }
    Ok(())
}

fn assert_parallel_group(
    automation: &AutomationV2Spec,
    parallel_node_ids: &[String],
) -> anyhow::Result<()> {
    if parallel_node_ids.len() < 2 {
        bail!("parallel authoring fixture must declare at least two concurrent nodes");
    }
    let nodes = parallel_node_ids
        .iter()
        .map(|node_id| {
            automation
                .flow
                .nodes
                .iter()
                .find(|node| node.node_id == *node_id)
                .with_context(|| format!("missing parallel node {node_id}"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let dependencies = nodes[0].depends_on.clone();
    let parallel_ids = parallel_node_ids.iter().collect::<HashSet<_>>();
    for node in nodes {
        if node.depends_on != dependencies {
            bail!("parallel nodes do not share the same readiness boundary");
        }
        if node
            .depends_on
            .iter()
            .any(|dependency| parallel_ids.contains(dependency))
        {
            bail!("parallel nodes depend on each other and would serialize");
        }
    }
    Ok(())
}

fn validate_authoritative_claim(
    assistant_claim: &str,
    tool_outcome: &Value,
    persisted: Option<&AutomationV2Spec>,
) -> anyhow::Result<()> {
    if assistant_claim.trim().is_empty() {
        bail!("assistant success claim is empty");
    }
    if tool_outcome.get("ok") != Some(&Value::Bool(true)) {
        bail!("assistant claimed success without a successful tool outcome");
    }
    let authoritative_id = tool_outcome
        .pointer("/resource/id")
        .and_then(Value::as_str)
        .context("successful tool outcome has no authoritative resource id")?;
    let persisted = persisted.context("assistant claimed success without a persisted artifact")?;
    if persisted.automation_id != authoritative_id {
        bail!("persisted artifact does not match the claimed resource");
    }
    Ok(())
}

fn assert_active_artifact(
    context: &Value,
    expected_id: &str,
    expected_revision: u32,
    forbidden_id: &str,
) -> anyhow::Result<()> {
    if context.get("selection") != Some(&json!("single_active")) {
        bail!("artifact selection is not single_active: {context}");
    }
    if context.pointer("/active/planner_session_id") != Some(&json!(expected_id)) {
        bail!("unexpected active artifact: {context}");
    }
    if context.pointer("/active/revision") != Some(&json!(expected_revision)) {
        bail!("active artifact revision is not continuous: {context}");
    }
    let recent = context
        .get("recent")
        .and_then(Value::as_array)
        .context("artifact context has no recent list")?;
    if recent
        .iter()
        .any(|row| row.get("planner_session_id").and_then(Value::as_str) == Some(forbidden_id))
    {
        bail!("foreign-tenant artifact leaked into chat context");
    }
    Ok(())
}

fn planner_session_fixture(
    tenant: &TenantContext,
    chat_session_id: &str,
    planner_session_id: &str,
    revision: u32,
    last_referenced_at_ms: u64,
) -> Value {
    let plan_id = format!("plan-{planner_session_id}");
    let plan = json!({
        "plan_id": plan_id,
        "planner_version": "agentic-authoring-eval-v1",
        "plan_source": "recorded_eval_fixture",
        "original_prompt": "Create a deterministic evaluation workflow",
        "normalized_prompt": "Create a deterministic evaluation workflow",
        "confidence": "recorded",
        "title": format!("Fixture {planner_session_id}"),
        "description": "Recorded planner artifact for active-reference evaluation",
        "schedule": {
            "type": "manual",
            "timezone": "UTC",
            "misfire_policy": { "type": "run_once" }
        },
        "execution_target": "automation_v2",
        "workspace_root": "/tmp/tandem-agentic-authoring-eval",
        "steps": [],
        "requires_integrations": [],
        "allowed_mcp_servers": [],
        "save_options": { "materialize_as_draft": true }
    });
    planner_session_fixture_with_plan(
        tenant,
        chat_session_id,
        planner_session_id,
        revision,
        last_referenced_at_ms,
        plan,
    )
}

fn planner_session_fixture_with_plan(
    tenant: &TenantContext,
    chat_session_id: &str,
    planner_session_id: &str,
    revision: u32,
    last_referenced_at_ms: u64,
    plan: Value,
) -> Value {
    let plan_id = plan
        .get("plan_id")
        .and_then(Value::as_str)
        .expect("planner fixture plan_id")
        .to_string();
    json!({
        "session_id": planner_session_id,
        "tenant_context": tenant,
        "linked_chat_session_id": chat_session_id,
        "linked_chat_run_id": format!("run-{chat_session_id}"),
        "last_referenced_at_ms": last_referenced_at_ms,
        "artifact_links": [],
        "project_slug": "agentic-authoring-eval",
        "title": format!("Fixture {planner_session_id}"),
        "workspace_root": "/tmp/tandem-agentic-authoring-eval",
        "source_kind": "agentic_chat",
        "current_plan_id": plan_id,
        "draft": {
            "initial_plan": plan,
            "current_plan": plan,
            "plan_revision": revision,
            "conversation": {
                "conversation_id": format!("conversation-{planner_session_id}"),
                "plan_id": plan_id,
                "created_at_ms": 1,
                "updated_at_ms": revision,
                "messages": []
            }
        },
        "goal": "Create a deterministic evaluation workflow",
        "notes": "",
        "planner_provider": "recorded-fixture",
        "planner_model": "recorded-fixture-v1",
        "plan_source": "agentic_chat",
        "allowed_mcp_servers": [],
        "import_transform_log": [],
        "published_tasks": [],
        "created_at_ms": 1,
        "updated_at_ms": last_referenced_at_ms
    })
}

fn eval_tenant(tenant_id: &str, actor_id: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(
        tenant_id,
        "agentic-authoring-eval",
        Some("ci".to_string()),
        actor_id,
    )
}

fn verified_context(
    tenant_context: TenantContext,
    actor_id: &str,
    roles: Vec<String>,
    capabilities: Vec<String>,
) -> VerifiedTenantContext {
    let principal = RequestPrincipal::authenticated_user(actor_id, "agentic-authoring-eval");
    VerifiedTenantContext {
        tenant_context,
        human_actor: HumanActor::tandem_user(actor_id),
        authority_chain: AuthorityChain::from_request(principal),
        roles,
        org_units: Vec::new(),
        capabilities,
        policy_version: None,
        strict_projection: None,
        issuer: "agentic-authoring-eval".to_string(),
        audience: "tandem".to_string(),
        issued_at_ms: 1,
        expires_at_ms: u64::MAX,
        assertion_id: format!("agentic-authoring-eval-{actor_id}"),
        assertion_key_id: None,
    }
}

async fn save_chat_session(
    state: &AppState,
    tenant: TenantContext,
    verified: VerifiedTenantContext,
    label: &str,
) -> anyhow::Result<Session> {
    let mut session = Session::new(
        Some(format!("Agentic authoring {label} eval")),
        Some(std::env::current_dir()?.to_string_lossy().to_string()),
    );
    session.tenant_context = tenant;
    session.verified_tenant_context = Some(verified);
    state.storage.save_session(session.clone()).await?;
    Ok(session)
}

fn dispatch_context(
    state: &AppState,
    tenant: &TenantContext,
    verified: &VerifiedTenantContext,
    session_id: &str,
    tool: &str,
    request_id: &str,
) -> tandem_tools::ToolDispatchContext {
    state
        .tool_dispatch_context(
            ToolDispatchSource::new("agentic_product_authoring_eval")
                .session(session_id)
                .message(request_id)
                .request(request_id),
            tenant.clone(),
            vec![tool.to_string()],
        )
        .with_verified_tenant_context(verified.clone())
}

async fn next_dispatch_event(
    receiver: &mut broadcast::Receiver<tandem_types::EngineEvent>,
    tool: &str,
) -> anyhow::Result<Value> {
    for _ in 0..64 {
        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .context("timed out waiting for tool dispatch audit event")??;
        if event.event_type == "tool.dispatch.recorded"
            && event.properties.get("tool").and_then(Value::as_str) == Some(tool)
            && matches!(
                event
                    .properties
                    .get("receipt_phase")
                    .and_then(Value::as_str),
                Some("execution_completed" | "execution_failed")
            )
        {
            return Ok(event.properties);
        }
    }
    bail!("no tool.dispatch.recorded event found for {tool}")
}

fn assert_dispatch_identity(
    event: &Value,
    session_id: &str,
    tenant_id: &str,
    expected_status: &str,
) -> anyhow::Result<()> {
    if event.pointer("/source/session_id") != Some(&json!(session_id))
        || event.pointer("/tenant_context/org_id") != Some(&json!(tenant_id))
        || event.get("status") != Some(&json!(expected_status))
    {
        bail!("tool dispatch audit attribution mismatch: {event}");
    }
    Ok(())
}

fn latest_assistant_text(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message.pointer("/info/role").and_then(Value::as_str) == Some("assistant"))
        .and_then(|message| message.get("parts").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn intent_name(intent: ToolIntent) -> &'static str {
    match intent {
        ToolIntent::Chitchat => "chitchat",
        ToolIntent::Knowledge => "knowledge",
        ToolIntent::WorkspaceRead => "workspace_read",
        ToolIntent::WorkspaceWrite => "workspace_write",
        ToolIntent::ShellExec => "shell_exec",
        ToolIntent::WebLookup => "web_lookup",
        ToolIntent::MemoryOps => "memory_ops",
        ToolIntent::McpExplicit => "mcp_explicit",
        ToolIntent::ProductAuthoring => "product_authoring",
        ToolIntent::ProductAuthoringWithMcp => "product_authoring_with_mcp",
        ToolIntent::ProductControl => "product_control",
    }
}

fn automation_status_name(status: &AutomationV2Status) -> &'static str {
    match status {
        AutomationV2Status::Active => "active",
        AutomationV2Status::Paused => "paused",
        AutomationV2Status::Draft => "draft",
    }
}

#[cfg(test)]
#[path = "agentic_product_authoring_tests.rs"]
mod tests;
