/// YAML → AutomationV2Spec Mapper
///
/// Translates an `EvalTestCase` (from the YAML dataset format) into the engine-native
/// `AutomationV2Spec` that `state.create_automation_v2_run()` consumes.
///
/// Used by `EngineExecutor` in `--engine-mode stub` and `--engine-mode live` runs of
/// the eval-runner CLI. Tests submit specs through this mapper and assert on the
/// resulting `AutomationV2RunRecord`.
///
/// Mapping rules:
/// - Each `TestNode.node_type` string is mapped to a stable
///   `AutomationOutputValidatorKind` and a contract `kind` label
/// - All nodes in a test case share a single default agent (`agent-1`)
/// - Nodes execute in parallel by default (no `depends_on`); test datasets can
///   later add an explicit dependency field if needed
/// - `max_repair_iterations` from the test case's config block is used for per-node
///   retry policy and (× 60s) for the per-node timeout
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};
use tandem_types::TenantContext;

use crate::eval::dataset::{EvalTestCase, TestNode};
use crate::{
    AutomationAgentMcpPolicy, AutomationAgentProfile, AutomationAgentToolPolicy,
    AutomationExecutionPolicy, AutomationFlowNode, AutomationFlowOutputContract,
    AutomationFlowSpec, AutomationOutputValidatorKind, AutomationV2Schedule,
    AutomationV2ScheduleType, AutomationV2Spec, AutomationV2Status, RoutineMisfirePolicy,
};

/// Single shared agent id for every node in a test-case-derived spec.
pub const EVAL_AGENT_ID: &str = "eval-agent-1";
/// Trigger string the executor records when submitting via `create_automation_v2_run`.
pub const EVAL_TRIGGER_TYPE: &str = "eval_runner";
/// Default per-node timeout multiplier — each repair iteration is given this many ms.
pub const PER_REPAIR_TIMEOUT_MS: u64 = 60_000;
/// Floor for the per-node timeout, even when `max_repair_iterations` is 0/1.
pub const MIN_NODE_TIMEOUT_MS: u64 = 60_000;
/// Default repair-iteration ceiling if a test case doesn't specify one.
pub const DEFAULT_MAX_REPAIR_ITERATIONS: u32 = 3;

/// Translate an `EvalTestCase` into a runnable `AutomationV2Spec`.
pub fn test_case_to_spec(case: &EvalTestCase) -> AutomationV2Spec {
    test_case_to_spec_with_options(case, EvalSpecOptions::default())
}

/// Translate an `EvalTestCase` into a local stub-mode `AutomationV2Spec`.
///
/// Stub specs include deterministic inline artifacts so the real runtime executor
/// and validators are exercised without making model/provider calls.
pub fn test_case_to_stub_spec(case: &EvalTestCase) -> AutomationV2Spec {
    test_case_to_spec_with_options(
        case,
        EvalSpecOptions {
            inline_artifacts: stub_case_should_use_inline_artifact(case),
        },
    )
}

#[derive(Debug, Clone, Copy, Default)]
struct EvalSpecOptions {
    inline_artifacts: bool,
}

fn test_case_to_spec_with_options(
    case: &EvalTestCase,
    options: EvalSpecOptions,
) -> AutomationV2Spec {
    let max_repair = effective_max_repair_iterations(case);
    let nodes = case
        .automation_spec
        .nodes
        .iter()
        .map(|n| map_node(case, n, max_repair, options))
        .collect();

    let now = current_time_ms();

    AutomationV2Spec {
        automation_id: format!("eval-{}", case.id),
        name: if case.automation_spec.name.is_empty() {
            format!("eval/{}", case.id)
        } else {
            case.automation_spec.name.clone()
        },
        description: Some(case.description.clone()),
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::Skip,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![default_agent()],
        flow: AutomationFlowSpec { nodes },
        execution: AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: Some(1),
            max_total_runtime_ms: Some(max_repair as u64 * PER_REPAIR_TIMEOUT_MS * 4),
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: EVAL_TRIGGER_TYPE.to_string(),
        workspace_root: None,
        metadata: eval_automation_metadata(case),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

fn stub_case_should_use_inline_artifact(case: &EvalTestCase) -> bool {
    if case
        .automation_spec
        .config
        .get("required_tool_calls")
        .is_some()
    {
        return false;
    }
    !case
        .automation_spec
        .config
        .get("builder")
        .and_then(Value::as_object)
        .and_then(|builder| {
            builder
                .get("task_class")
                .or_else(|| builder.get("task_kind"))
        })
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("connector_preflight"))
}

fn map_node(
    case: &EvalTestCase,
    node: &TestNode,
    max_repair: u32,
    options: EvalSpecOptions,
) -> AutomationFlowNode {
    let validator = validator_for_node_type(&node.node_type);
    let kind_label = contract_kind_for_node_type(&node.node_type);
    let summary_guidance = if node.output_contract.is_empty() {
        None
    } else {
        Some(node.output_contract.clone())
    };

    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node.id.clone(),
        agent_id: EVAL_AGENT_ID.to_string(),
        objective: node.objective.clone(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: kind_label.to_string(),
            validator: Some(validator),
            enforcement: None,
            schema: None,
            summary_guidance,
        }),
        tool_policy: None,
        mcp_policy: None,
        retry_policy: Some(json!({
            "max_attempts": max_repair,
            "retries": max_repair.saturating_sub(1),
        })),
        timeout_ms: Some(node_timeout_ms(max_repair)),
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: eval_node_metadata(case, node, options),
    }
}

fn eval_automation_metadata(case: &EvalTestCase) -> Option<Value> {
    let mut metadata = Map::new();
    copy_config_string(
        &case.automation_spec.config,
        &mut metadata,
        "runtime_profile",
    );
    copy_config_string(
        &case.automation_spec.config,
        &mut metadata,
        "domain_profile",
    );
    copy_config_string(
        &case.automation_spec.config,
        &mut metadata,
        "fintech_profile",
    );
    copy_config_string(&case.automation_spec.config, &mut metadata, "tenant_id");
    copy_eval_tenant_context(case, &mut metadata);
    if metadata_enables_eval_fintech_strict(&metadata) {
        metadata
            .entry("runtime_profile".to_string())
            .or_insert_with(|| Value::String(tandem_core::FINTECH_STRICT_PROFILE.to_string()));
        metadata
            .entry("domain_profile".to_string())
            .or_insert_with(|| Value::String(tandem_core::FINTECH_STRICT_PROFILE.to_string()));
        metadata
            .entry("fintech_profile".to_string())
            .or_insert_with(|| Value::String(tandem_core::FINTECH_STRICT_PROFILE.to_string()));
        metadata
            .entry("fintech_strict".to_string())
            .or_insert(Value::Bool(true));
    }
    if metadata.is_empty() {
        None
    } else {
        Some(Value::Object(metadata))
    }
}

fn eval_node_metadata(
    case: &EvalTestCase,
    node: &TestNode,
    options: EvalSpecOptions,
) -> Option<Value> {
    let mut metadata = Map::new();
    copy_config_string(
        &case.automation_spec.config,
        &mut metadata,
        "artifact_contract",
    );
    copy_config_string(&case.automation_spec.config, &mut metadata, "artifact_type");
    copy_config_value(&case.automation_spec.config, &mut metadata, "allowed_tools");
    copy_config_value(
        &case.automation_spec.config,
        &mut metadata,
        "required_tool_calls",
    );
    copy_config_value(&case.automation_spec.config, &mut metadata, "builder");
    if let Some(contract) = metadata
        .get("artifact_contract")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        let mut fintech = Map::new();
        fintech.insert("artifact_contract".to_string(), Value::String(contract));
        metadata.insert("fintech".to_string(), Value::Object(fintech));
    }
    if case
        .automation_spec
        .config
        .get("runtime_profile")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(tandem_core::FINTECH_STRICT_PROFILE))
    {
        metadata.insert("fintech_strict".to_string(), Value::Bool(true));
    }
    let mut eval = json!({
        "test_id": case.id,
        "node_type": node.node_type,
    });
    if options.inline_artifacts {
        if let Some(eval_object) = eval.as_object_mut() {
            eval_object.insert(
                "inline_artifact".to_string(),
                eval_inline_artifact_payload(case, node),
            );
        }
    }
    metadata.insert("eval".to_string(), eval);
    if metadata.is_empty() {
        None
    } else {
        Some(Value::Object(metadata))
    }
}

fn eval_inline_artifact_payload(case: &EvalTestCase, node: &TestNode) -> Value {
    json!({
        "status": "completed",
        "summary": format!(
            "Scripted eval artifact for `{}` in `{}`: {}",
            node.id, case.id, node.objective
        ),
        "content": format!(
            "Deterministic stub output for `{}` satisfying `{}`.",
            node.objective, node.output_contract
        ),
        "decision": "pass",
        "category": node.node_type,
        "results": {
            "passed": true,
            "node_id": node.id,
            "test_id": case.id
        },
        "test_results": {
            "passed": true,
            "errors": []
        },
        "available_sources": [
            "scripted-eval-fixture"
        ],
        "data_quality": "sufficient",
        "sources": [
            "https://example.com/scripted-eval/source-1",
            "https://example.com/scripted-eval/source-2"
        ],
        "citations": [
            "https://example.com/scripted-eval/source-1",
            "https://example.com/scripted-eval/source-2"
        ],
        "web_sources": [
            "https://example.com/scripted-eval/source-1",
            "https://example.com/scripted-eval/source-2"
        ],
        "web_sources_reviewed": [
            "https://example.com/scripted-eval/source-1",
            "https://example.com/scripted-eval/source-2"
        ],
        "code": "def parse_json(value):\n    import json\n    return json.loads(value)",
        "markdown": "## Summary\n\n- Scripted eval output\n- Deterministic stub artifact",
        "key_points": [
            "Scripted eval output",
            "Deterministic stub artifact"
        ]
    })
}

fn metadata_enables_eval_fintech_strict(metadata: &Map<String, Value>) -> bool {
    let value = Value::Object(metadata.clone());
    tandem_core::metadata_enables_fintech_strict(Some(&value))
}

fn copy_config_string(
    config: &std::collections::HashMap<String, Value>,
    metadata: &mut Map<String, Value>,
    key: &str,
) {
    if let Some(value) = config.get(key).and_then(Value::as_str).map(str::trim) {
        if !value.is_empty() {
            metadata.insert(key.to_string(), Value::String(value.to_string()));
        }
    }
}

fn copy_config_value(
    config: &std::collections::HashMap<String, Value>,
    metadata: &mut Map<String, Value>,
    key: &str,
) {
    if let Some(value) = config.get(key) {
        metadata.insert(key.to_string(), value.clone());
    }
}

fn copy_eval_tenant_context(case: &EvalTestCase, metadata: &mut Map<String, Value>) {
    if let Some(value) = case.automation_spec.config.get("tenant_context") {
        metadata.insert("tenant_context".to_string(), value.clone());
        return;
    }
    let Some(tenant_id) = case
        .automation_spec
        .config
        .get("tenant_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let tenant_context = TenantContext::explicit_user_workspace(
        tenant_id,
        "eval-workspace",
        Some("eval-deployment".to_string()),
        format!("{tenant_id}-eval-actor"),
    );
    metadata.insert(
        "tenant_context".to_string(),
        serde_json::to_value(tenant_context).unwrap_or(Value::Null),
    );
}

fn default_agent() -> AutomationAgentProfile {
    AutomationAgentProfile {
        agent_id: EVAL_AGENT_ID.to_string(),
        template_id: None,
        display_name: "Eval Worker".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: AutomationAgentToolPolicy {
            allowlist: Vec::new(),
            denylist: Vec::new(),
        },
        mcp_policy: AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    }
}

/// Map a free-form node_type string to a `AutomationOutputValidatorKind` variant.
///
/// Unknown types fall through to `GenericArtifact`, which is the engine's catch-all
/// validator and lets evals declare new node-type labels without code changes here.
pub fn validator_for_node_type(node_type: &str) -> AutomationOutputValidatorKind {
    let lower = node_type.to_ascii_lowercase();
    match lower.as_str() {
        "research" | "research_synthesis" | "web_research" | "report" => {
            AutomationOutputValidatorKind::ResearchBrief
        }
        "code" | "code_generation" | "code_patch" | "patch" => {
            AutomationOutputValidatorKind::CodePatch
        }
        "review" | "review_decision" | "decision" | "approval" => {
            AutomationOutputValidatorKind::ReviewDecision
        }
        "generation" | "summarization" | "structured" | "structured_json" | "json" => {
            AutomationOutputValidatorKind::StructuredJson
        }
        "standup" | "standup_update" => AutomationOutputValidatorKind::StandupUpdate,
        _ => AutomationOutputValidatorKind::GenericArtifact,
    }
}

/// Map a node_type to a contract `kind` string (free-form label, used by the engine
/// and downstream surfaces). Aligned with `validator_for_node_type` but lossy: any
/// research-flavored type collapses to "report", any code type to "code", etc.
pub fn contract_kind_for_node_type(node_type: &str) -> &'static str {
    match validator_for_node_type(node_type) {
        AutomationOutputValidatorKind::ResearchBrief => "report",
        AutomationOutputValidatorKind::CodePatch => "code",
        AutomationOutputValidatorKind::ReviewDecision => "decision",
        AutomationOutputValidatorKind::StructuredJson => "structured",
        AutomationOutputValidatorKind::GenericArtifact => "artifact",
        AutomationOutputValidatorKind::StandupUpdate => "standup",
    }
}

fn effective_max_repair_iterations(case: &EvalTestCase) -> u32 {
    // Prefer the test case's config block; fall back to expected_output; finally the default.
    let from_config = case
        .automation_spec
        .config
        .get("max_repair_iterations")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let from_expected = case.expected_output.max_repair_iterations;

    from_config
        .or(from_expected)
        .unwrap_or(DEFAULT_MAX_REPAIR_ITERATIONS)
        .max(1)
}

fn node_timeout_ms(max_repair: u32) -> u64 {
    (max_repair as u64 * PER_REPAIR_TIMEOUT_MS).max(MIN_NODE_TIMEOUT_MS)
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::dataset::{
        ArtifactStatus, AutomationSpecTest, EvalExpectedOutput, EvalTestCase, MetricTolerance,
        TestNode,
    };
    use std::collections::HashMap;

    fn make_case(id: &str, nodes: Vec<TestNode>) -> EvalTestCase {
        EvalTestCase {
            id: id.to_string(),
            description: format!("desc for {}", id),
            priority: 1,
            automation_spec: AutomationSpecTest {
                name: format!("automation-{}", id),
                nodes,
                validators: vec!["contract".to_string()],
                config: HashMap::new(),
            },
            expected_output: EvalExpectedOutput {
                artifact_status: ArtifactStatus::Completed,
                required_validators: vec!["contract".to_string()],
                optional_validators: Vec::new(),
                unmet_requirements_acceptable: false,
                max_repair_iterations: Some(2),
                output_format: "json".to_string(),
                quality_indicators: Vec::new(),
            },
            enabled: true,
            tags: vec!["test".to_string()],
            metric_tolerance: MetricTolerance::default(),
        }
    }

    fn make_node(id: &str, node_type: &str) -> TestNode {
        TestNode {
            id: id.to_string(),
            node_type: node_type.to_string(),
            objective: format!("Do {}", node_type),
            output_contract: format!("Produce a {} output", node_type),
        }
    }

    #[test]
    fn validator_mapping_covers_all_eval_dataset_node_types() {
        // Every node_type used in eval_datasets/*.yaml must map deterministically.
        // The catch-all guarantees no panic, but we want the named mappings to be stable.
        assert_eq!(
            validator_for_node_type("research"),
            AutomationOutputValidatorKind::ResearchBrief
        );
        assert_eq!(
            validator_for_node_type("research_synthesis"),
            AutomationOutputValidatorKind::ResearchBrief
        );
        assert_eq!(
            validator_for_node_type("code"),
            AutomationOutputValidatorKind::CodePatch
        );
        assert_eq!(
            validator_for_node_type("generation"),
            AutomationOutputValidatorKind::StructuredJson
        );
        assert_eq!(
            validator_for_node_type("summarization"),
            AutomationOutputValidatorKind::StructuredJson
        );
        assert_eq!(
            validator_for_node_type("review"),
            AutomationOutputValidatorKind::ReviewDecision
        );
        assert_eq!(
            validator_for_node_type("standup_update"),
            AutomationOutputValidatorKind::StandupUpdate
        );
        // Catch-all for unknown labels
        assert_eq!(
            validator_for_node_type("totally-new-thing"),
            AutomationOutputValidatorKind::GenericArtifact
        );
    }

    #[test]
    fn node_type_matching_is_case_insensitive() {
        assert_eq!(
            validator_for_node_type("Research"),
            AutomationOutputValidatorKind::ResearchBrief
        );
        assert_eq!(
            validator_for_node_type("CODE_GENERATION"),
            AutomationOutputValidatorKind::CodePatch
        );
    }

    #[test]
    fn contract_kind_matches_validator_family() {
        assert_eq!(contract_kind_for_node_type("research"), "report");
        assert_eq!(contract_kind_for_node_type("code"), "code");
        assert_eq!(contract_kind_for_node_type("review"), "decision");
        assert_eq!(contract_kind_for_node_type("generation"), "structured");
        assert_eq!(contract_kind_for_node_type("unknown_type"), "artifact");
    }

    #[test]
    fn produces_valid_spec_with_single_node() {
        let case = make_case("ev_001", vec![make_node("n1", "research")]);
        let spec = test_case_to_spec(&case);

        assert_eq!(spec.automation_id, "eval-ev_001");
        assert_eq!(spec.flow.nodes.len(), 1);
        assert_eq!(spec.agents.len(), 1);
        assert_eq!(spec.agents[0].agent_id, EVAL_AGENT_ID);
        assert!(matches!(spec.status, AutomationV2Status::Active));
        assert!(matches!(
            spec.schedule.schedule_type,
            AutomationV2ScheduleType::Manual
        ));

        let node = &spec.flow.nodes[0];
        assert_eq!(node.node_id, "n1");
        assert_eq!(node.agent_id, EVAL_AGENT_ID);
        let contract = node.output_contract.as_ref().expect("contract present");
        assert_eq!(
            contract.validator,
            Some(AutomationOutputValidatorKind::ResearchBrief)
        );
        assert_eq!(contract.kind, "report");
        assert_eq!(
            contract.summary_guidance.as_deref(),
            Some("Produce a research output")
        );
    }

    #[test]
    fn default_eval_nodes_omit_deterministic_inline_artifacts() {
        let case = make_case("ev_live", vec![make_node("research_node", "research")]);
        let spec = test_case_to_spec(&case);
        let node_metadata = spec.flow.nodes[0].metadata.as_ref().expect("node metadata");
        let eval_metadata = node_metadata.get("eval").expect("eval metadata");

        assert_eq!(
            eval_metadata.get("test_id").and_then(Value::as_str),
            Some("ev_live")
        );
        assert_eq!(
            eval_metadata.get("node_type").and_then(Value::as_str),
            Some("research")
        );
        assert!(eval_metadata.get("inline_artifact").is_none());
    }

    #[test]
    fn stub_eval_nodes_include_deterministic_inline_artifact_metadata() {
        let case = make_case("ev_inline", vec![make_node("research_node", "research")]);
        let spec = test_case_to_stub_spec(&case);
        let node_metadata = spec.flow.nodes[0].metadata.as_ref().expect("node metadata");
        let eval_metadata = node_metadata.get("eval").expect("eval metadata");
        let inline_artifact = eval_metadata
            .get("inline_artifact")
            .expect("inline artifact payload");

        assert_eq!(
            inline_artifact.get("status").and_then(Value::as_str),
            Some("completed")
        );
        assert_eq!(
            inline_artifact
                .get("results")
                .and_then(|results| results.get("node_id"))
                .and_then(Value::as_str),
            Some("research_node")
        );
        assert!(inline_artifact.get("citations").is_some());
        assert!(inline_artifact.get("code").is_some());
    }

    #[test]
    fn stub_preflight_eval_nodes_do_not_use_inline_artifact_shortcut() {
        let mut case = make_case("ev_preflight_stub", vec![make_node("n1", "research")]);
        case.automation_spec.config.insert(
            "required_tool_calls".to_string(),
            json!([{"tool": "eval.tenant_resource_probe"}]),
        );
        case.automation_spec.config.insert(
            "builder".to_string(),
            json!({"task_class": "connector_preflight"}),
        );

        let spec = test_case_to_stub_spec(&case);
        let node_metadata = spec.flow.nodes[0].metadata.as_ref().expect("node metadata");
        let eval_metadata = node_metadata.get("eval").expect("eval metadata");

        assert!(eval_metadata.get("inline_artifact").is_none());
    }

    #[test]
    fn produces_valid_spec_with_multiple_nodes() {
        let case = make_case(
            "ev_002",
            vec![
                make_node("step1", "research"),
                make_node("step2", "code"),
                make_node("step3", "summarization"),
            ],
        );
        let spec = test_case_to_spec(&case);
        assert_eq!(spec.flow.nodes.len(), 3);

        let validators: Vec<_> = spec
            .flow
            .nodes
            .iter()
            .map(|n| n.output_contract.as_ref().unwrap().validator.unwrap())
            .collect();
        assert_eq!(
            validators,
            vec![
                AutomationOutputValidatorKind::ResearchBrief,
                AutomationOutputValidatorKind::CodePatch,
                AutomationOutputValidatorKind::StructuredJson,
            ]
        );

        // Nodes execute in parallel by default — no depends_on.
        for node in &spec.flow.nodes {
            assert!(node.depends_on.is_empty());
        }
    }

    #[test]
    fn config_max_repair_overrides_expected_output() {
        let mut case = make_case("ev_003", vec![make_node("n1", "research")]);
        case.automation_spec
            .config
            .insert("max_repair_iterations".to_string(), serde_json::json!(5));
        case.expected_output.max_repair_iterations = Some(2);

        let spec = test_case_to_spec(&case);
        let retry = spec.flow.nodes[0]
            .retry_policy
            .as_ref()
            .expect("retry_policy present");
        assert_eq!(retry["max_attempts"], 5);
        assert_eq!(retry["retries"], 4);

        // Timeout scales with the effective max_repair.
        assert_eq!(
            spec.flow.nodes[0].timeout_ms,
            Some(5 * PER_REPAIR_TIMEOUT_MS)
        );
    }

    #[test]
    fn config_runtime_profile_stamps_fintech_metadata() {
        let mut case = make_case("fintech_001", vec![make_node("n1", "structured_json")]);
        case.automation_spec.config.insert(
            "runtime_profile".to_string(),
            serde_json::json!("fintech_strict"),
        );
        case.automation_spec.config.insert(
            "artifact_contract".to_string(),
            serde_json::json!("compliance_risk_update_brief"),
        );
        case.automation_spec
            .config
            .insert("tenant_id".to_string(), serde_json::json!("tenant-a"));

        let spec = test_case_to_spec(&case);

        let metadata = spec.metadata.as_ref().expect("automation metadata");
        assert!(tandem_core::metadata_enables_fintech_strict(Some(metadata)));
        assert_eq!(
            metadata.get("tenant_id").and_then(Value::as_str),
            Some("tenant-a")
        );
        assert_eq!(
            metadata
                .get("tenant_context")
                .and_then(|value| value.get("org_id"))
                .and_then(Value::as_str),
            Some("tenant-a")
        );
        let node_metadata = spec.flow.nodes[0].metadata.as_ref().expect("node metadata");
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
    fn falls_back_to_expected_output_when_config_missing() {
        let case = make_case("ev_004", vec![make_node("n1", "research")]);
        // make_case sets expected_output.max_repair_iterations = Some(2) and leaves config empty.
        let spec = test_case_to_spec(&case);
        let retry = spec.flow.nodes[0].retry_policy.as_ref().unwrap();
        assert_eq!(retry["max_attempts"], 2);
        assert_eq!(retry["retries"], 1);
        assert_eq!(
            spec.flow.nodes[0].timeout_ms,
            Some(2 * PER_REPAIR_TIMEOUT_MS)
        );
    }

    #[test]
    fn falls_back_to_default_when_neither_specified() {
        let mut case = make_case("ev_005", vec![make_node("n1", "research")]);
        case.expected_output.max_repair_iterations = None;
        let spec = test_case_to_spec(&case);
        let retry = spec.flow.nodes[0].retry_policy.as_ref().unwrap();
        assert_eq!(retry["max_attempts"], DEFAULT_MAX_REPAIR_ITERATIONS);
    }

    #[test]
    fn timeout_floor_applies_when_max_repair_is_one() {
        let mut case = make_case("ev_006", vec![make_node("n1", "research")]);
        case.expected_output.max_repair_iterations = Some(1);
        let spec = test_case_to_spec(&case);
        // Floor: 1 * 60_000 == 60_000, also >= MIN_NODE_TIMEOUT_MS
        assert_eq!(spec.flow.nodes[0].timeout_ms, Some(MIN_NODE_TIMEOUT_MS));
    }

    #[test]
    fn empty_objective_contract_yields_none_summary_guidance() {
        let case = make_case(
            "ev_007",
            vec![TestNode {
                id: "n1".to_string(),
                node_type: "research".to_string(),
                objective: "Investigate".to_string(),
                output_contract: String::new(),
            }],
        );
        let spec = test_case_to_spec(&case);
        let contract = spec.flow.nodes[0].output_contract.as_ref().unwrap();
        assert_eq!(contract.summary_guidance, None);
    }

    #[test]
    fn empty_automation_name_falls_back_to_eval_id() {
        let mut case = make_case("ev_008", vec![make_node("n1", "research")]);
        case.automation_spec.name = String::new();
        let spec = test_case_to_spec(&case);
        assert_eq!(spec.name, "eval/ev_008");
    }

    #[test]
    fn execution_policy_has_single_agent_and_runtime_cap() {
        let case = make_case("ev_009", vec![make_node("n1", "research")]);
        let spec = test_case_to_spec(&case);
        assert_eq!(spec.execution.max_parallel_agents, Some(1));
        assert!(spec.execution.max_total_runtime_ms.unwrap() > 0);
        assert_eq!(spec.execution.profile, None);
    }

    #[test]
    fn eval_config_copies_preflight_metadata_to_nodes() {
        let mut case = make_case("ev_preflight", vec![make_node("n1", "research")]);
        case.automation_spec.config.insert(
            "allowed_tools".to_string(),
            json!(["eval.tenant_resource_probe"]),
        );
        case.automation_spec.config.insert(
            "required_tool_calls".to_string(),
            json!([{
                "tool": "eval.tenant_resource_probe",
                "args": {
                    "resource_key": "project/eval/ct02-tenant-b-source",
                    "attempted_tenant_id": "tenant-b"
                }
            }]),
        );
        case.automation_spec.config.insert(
            "builder".to_string(),
            json!({
                "task_class": "connector_preflight",
                "output_path": ".tandem/eval/ct02.json"
            }),
        );

        let spec = test_case_to_spec(&case);
        let metadata = spec.flow.nodes[0].metadata.as_ref().expect("metadata");

        assert_eq!(
            metadata
                .pointer("/builder/task_class")
                .and_then(Value::as_str),
            Some("connector_preflight")
        );
        assert!(metadata.get("required_tool_calls").is_some());
        assert!(metadata.get("allowed_tools").is_some());
    }
}
