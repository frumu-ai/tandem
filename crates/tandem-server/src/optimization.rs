use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::{AutomationV2RunRecord, AutomationV2Spec};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationTargetKind {
    WorkflowV2PromptObjectiveOptimization,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationCampaignStatus {
    Draft,
    Running,
    AwaitingPromotionApproval,
    PausedManual,
    PausedBudget,
    PausedEvaluatorUnstable,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationExperimentStatus {
    Draft,
    Completed,
    PromotionRecommended,
    PromotionApproved,
    PromotionRejected,
    Discarded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OptimizationArtifactRefs {
    pub objective_ref: String,
    pub eval_ref: String,
    pub mutation_policy_ref: String,
    pub scope_ref: String,
    pub budget_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research_log_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationFrozenArtifact {
    pub artifact_ref: String,
    pub resolved_path: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationFrozenArtifacts {
    pub objective: OptimizationFrozenArtifact,
    pub eval: OptimizationFrozenArtifact,
    pub mutation_policy: OptimizationFrozenArtifact,
    pub scope: OptimizationFrozenArtifact,
    pub budget: OptimizationFrozenArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptimizationExecutionOverride {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationMetricKind {
    ArtifactValidatorPassRate,
    UnmetRequirementCount,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationGuardrailKind {
    BlockedNodeRate,
    BudgetCeilings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationMutableField {
    Objective,
    OutputContractSummaryGuidance,
    TimeoutMs,
    RetryPolicyMaxAttempts,
    RetryPolicyRetries,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationEvalSpec {
    pub pack_ref: String,
    pub primary_metric: OptimizationMetricKind,
    pub secondary_metric: OptimizationMetricKind,
    #[serde(default)]
    pub hard_guardrails: Vec<OptimizationGuardrailKind>,
    pub campaign_start_baseline_runs: u32,
    pub baseline_replay_every_candidates: u32,
    pub baseline_replay_every_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationMutationPolicy {
    pub max_nodes_changed_per_candidate: u32,
    pub max_field_families_changed_per_candidate: u32,
    #[serde(default)]
    pub allowed_text_fields: Vec<OptimizationMutableField>,
    #[serde(default)]
    pub allowed_knob_fields: Vec<OptimizationMutableField>,
    pub max_text_delta_chars: u32,
    pub max_text_delta_ratio: f64,
    pub timeout_delta_percent: f64,
    pub timeout_delta_ms: u64,
    pub timeout_min_ms: u64,
    pub timeout_max_ms: u64,
    pub retry_delta: i32,
    pub retry_min: i32,
    pub retry_max: i32,
    #[serde(default)]
    pub allow_text_and_knob_bundle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationSafetyScope {
    pub candidate_snapshot_only: bool,
    pub allow_live_source_mutation: bool,
    pub allow_external_side_effects_in_eval: bool,
    pub promotion_requires_operator_approval: bool,
    #[serde(default)]
    pub forbidden_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationBudgetPolicy {
    pub max_experiments: u32,
    pub max_runtime_minutes: u32,
    pub max_consecutive_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationPhase1Config {
    pub objective_markdown: String,
    pub eval: OptimizationEvalSpec,
    pub mutation_policy: OptimizationMutationPolicy,
    pub scope: OptimizationSafetyScope,
    pub budget: OptimizationBudgetPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OptimizationPhase1Metrics {
    pub artifact_validator_pass_rate: f64,
    pub unmet_requirement_count: f64,
    pub blocked_node_rate: f64,
    pub budget_within_limits: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OptimizationBaselineReplayRecord {
    pub replay_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_run_id: Option<String>,
    pub phase1_metrics: OptimizationPhase1Metrics,
    #[serde(default)]
    pub validator_case_outcomes: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub experiment_count_at_recording: u64,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationPromotionDecisionKind {
    Promote,
    Discard,
    NeedsOperatorReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptimizationPromotionDecision {
    pub decision: OptimizationPromotionDecisionKind,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptimizationValidatedMutation {
    pub node_id: String,
    pub field: OptimizationMutableField,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationCampaignRecord {
    pub optimization_id: String,
    pub name: String,
    pub target_kind: OptimizationTargetKind,
    pub status: OptimizationCampaignStatus,
    pub source_workflow_id: String,
    pub source_workflow_name: String,
    pub source_workflow_snapshot: AutomationV2Spec,
    pub source_workflow_snapshot_hash: String,
    pub baseline_snapshot: AutomationV2Spec,
    pub baseline_snapshot_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_override: Option<OptimizationExecutionOverride>,
    pub artifacts: OptimizationArtifactRefs,
    pub frozen_artifacts: OptimizationFrozenArtifacts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase1: Option<OptimizationPhase1Config>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_metrics: Option<OptimizationPhase1Metrics>,
    #[serde(default)]
    pub baseline_replays: Vec<OptimizationBaselineReplayRecord>,
    #[serde(default)]
    pub pending_baseline_run_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_promotion_experiment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pause_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationExperimentRecord {
    pub experiment_id: String,
    pub optimization_id: String,
    pub status: OptimizationExperimentStatus,
    pub candidate_snapshot: AutomationV2Spec,
    pub candidate_snapshot_hash: String,
    pub baseline_snapshot_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase1_metrics: Option<OptimizationPhase1Metrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion_recommendation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion_decision: Option<OptimizationPromotionDecision>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

pub fn optimization_snapshot_hash(snapshot: &AutomationV2Spec) -> String {
    let canonical = serde_json::to_vec(snapshot).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    format!("{:x}", hasher.finalize())
}

pub fn apply_optimization_execution_override(
    workflow: &AutomationV2Spec,
    execution_override: &OptimizationExecutionOverride,
) -> AutomationV2Spec {
    let mut snapshot = workflow.clone();
    for agent in &mut snapshot.agents {
        let mut policy = agent
            .model_policy
            .clone()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        let fixed_model = serde_json::json!({
            "provider_id": execution_override.provider_id,
            "model_id": execution_override.model_id,
        });
        policy.insert("default_model".to_string(), fixed_model.clone());
        if let Some(role_models) = policy.get_mut("role_models").and_then(Value::as_object_mut) {
            for role_model in role_models.values_mut() {
                *role_model = fixed_model.clone();
            }
        }
        agent.model_policy = Some(Value::Object(policy));
    }
    snapshot
}

pub fn freeze_optimization_artifact(
    workspace_root: &str,
    artifact_ref: &str,
) -> Result<OptimizationFrozenArtifact, String> {
    let trimmed = artifact_ref.trim();
    if trimmed.is_empty() {
        return Err("artifact ref is required".to_string());
    }
    let workspace = PathBuf::from(workspace_root);
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    if !tandem_core::is_within_workspace_root(&resolved, &workspace) {
        return Err(format!(
            "artifact `{trimmed}` must stay within workspace root `{workspace_root}`"
        ));
    }
    let metadata = std::fs::metadata(&resolved)
        .map_err(|_| format!("artifact `{trimmed}` does not exist or is unreadable"))?;
    if !metadata.is_file() {
        return Err(format!("artifact `{trimmed}` must be a file"));
    }
    let bytes =
        std::fs::read(&resolved).map_err(|_| format!("artifact `{trimmed}` could not be read"))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let resolved_path = resolved
        .canonicalize()
        .unwrap_or_else(|_| Path::new(&resolved).to_path_buf());
    Ok(OptimizationFrozenArtifact {
        artifact_ref: trimmed.to_string(),
        resolved_path: resolved_path.to_string_lossy().to_string(),
        sha256: format!("{:x}", hasher.finalize()),
        size_bytes: metadata.len(),
    })
}

fn read_optimization_artifact_text(
    artifact: &OptimizationFrozenArtifact,
    label: &str,
) -> Result<String, String> {
    std::fs::read_to_string(&artifact.resolved_path).map_err(|_| {
        format!(
            "{label} artifact `{}` could not be read as UTF-8 text",
            artifact.artifact_ref
        )
    })
}

fn parse_yaml_artifact<T: for<'de> Deserialize<'de>>(
    artifact: &OptimizationFrozenArtifact,
    label: &str,
) -> Result<T, String> {
    let raw = read_optimization_artifact_text(artifact, label)?;
    serde_yaml::from_str::<T>(&raw).map_err(|error| {
        format!(
            "failed to parse {label} artifact `{}`: {error}",
            artifact.artifact_ref
        )
    })
}

fn validate_phase1_eval_spec(eval: &OptimizationEvalSpec) -> Result<(), String> {
    if eval.pack_ref.trim().is_empty() {
        return Err("eval pack_ref is required".to_string());
    }
    if eval.primary_metric != OptimizationMetricKind::ArtifactValidatorPassRate {
        return Err("phase 1 eval.primary_metric must be artifact_validator_pass_rate".to_string());
    }
    if eval.secondary_metric != OptimizationMetricKind::UnmetRequirementCount {
        return Err("phase 1 eval.secondary_metric must be unmet_requirement_count".to_string());
    }
    if eval.hard_guardrails.len() != 2
        || !eval
            .hard_guardrails
            .contains(&OptimizationGuardrailKind::BlockedNodeRate)
        || !eval
            .hard_guardrails
            .contains(&OptimizationGuardrailKind::BudgetCeilings)
    {
        return Err(
            "phase 1 eval.hard_guardrails must be exactly blocked_node_rate and budget_ceilings"
                .to_string(),
        );
    }
    if eval.campaign_start_baseline_runs != 2 {
        return Err("phase 1 campaign_start_baseline_runs must be 2".to_string());
    }
    if eval.baseline_replay_every_candidates != 5 {
        return Err("phase 1 baseline_replay_every_candidates must be 5".to_string());
    }
    if eval.baseline_replay_every_minutes != 30 {
        return Err("phase 1 baseline_replay_every_minutes must be 30".to_string());
    }
    Ok(())
}

fn validate_phase1_mutation_policy(policy: &OptimizationMutationPolicy) -> Result<(), String> {
    if policy.max_nodes_changed_per_candidate != 1 {
        return Err("phase 1 max_nodes_changed_per_candidate must be 1".to_string());
    }
    if policy.max_field_families_changed_per_candidate != 1 {
        return Err("phase 1 max_field_families_changed_per_candidate must be 1".to_string());
    }
    if policy.allowed_text_fields.is_empty() && policy.allowed_knob_fields.is_empty() {
        return Err("phase 1 mutation policy must allow at least one mutable field".to_string());
    }
    if policy.allowed_text_fields.iter().any(|field| {
        !matches!(
            field,
            OptimizationMutableField::Objective
                | OptimizationMutableField::OutputContractSummaryGuidance
        )
    }) {
        return Err(
            "phase 1 allowed_text_fields may only include objective or output_contract_summary_guidance"
                .to_string(),
        );
    }
    if policy.allowed_knob_fields.iter().any(|field| {
        !matches!(
            field,
            OptimizationMutableField::TimeoutMs
                | OptimizationMutableField::RetryPolicyMaxAttempts
                | OptimizationMutableField::RetryPolicyRetries
        )
    }) {
        return Err(
            "phase 1 allowed_knob_fields may only include timeout_ms, retry_policy_max_attempts, or retry_policy_retries"
                .to_string(),
        );
    }
    if policy.max_text_delta_chars == 0 || policy.max_text_delta_chars > 300 {
        return Err("phase 1 max_text_delta_chars must be between 1 and 300".to_string());
    }
    if !(0.0 < policy.max_text_delta_ratio && policy.max_text_delta_ratio <= 0.25) {
        return Err("phase 1 max_text_delta_ratio must be > 0 and <= 0.25".to_string());
    }
    if !(0.0 < policy.timeout_delta_percent && policy.timeout_delta_percent <= 0.15) {
        return Err("phase 1 timeout_delta_percent must be > 0 and <= 0.15".to_string());
    }
    if policy.timeout_delta_ms == 0 || policy.timeout_delta_ms > 30_000 {
        return Err("phase 1 timeout_delta_ms must be between 1 and 30000".to_string());
    }
    if policy.timeout_min_ms < 30_000
        || policy.timeout_max_ms > 600_000
        || policy.timeout_min_ms >= policy.timeout_max_ms
    {
        return Err(
            "phase 1 timeout bounds must stay within 30000..600000 ms and min < max".to_string(),
        );
    }
    if policy.retry_delta <= 0 || policy.retry_delta > 1 {
        return Err("phase 1 retry_delta must be 1".to_string());
    }
    if policy.retry_min < 0 || policy.retry_max > 3 || policy.retry_min > policy.retry_max {
        return Err("phase 1 retry bounds must stay within 0..3".to_string());
    }
    if policy.allow_text_and_knob_bundle {
        return Err("phase 1 may not bundle text and knob mutations".to_string());
    }
    Ok(())
}

fn validate_phase1_scope(scope: &OptimizationSafetyScope) -> Result<(), String> {
    if !scope.candidate_snapshot_only {
        return Err("phase 1 scope must require candidate_snapshot_only".to_string());
    }
    if scope.allow_live_source_mutation {
        return Err("phase 1 scope must forbid live source mutation".to_string());
    }
    if scope.allow_external_side_effects_in_eval {
        return Err("phase 1 scope must forbid external side effects in eval".to_string());
    }
    if !scope.promotion_requires_operator_approval {
        return Err("phase 1 scope must require operator approval for promotion".to_string());
    }
    Ok(())
}

fn validate_phase1_budget(budget: &OptimizationBudgetPolicy) -> Result<(), String> {
    if budget.max_experiments == 0 || budget.max_experiments > 100 {
        return Err("phase 1 budget.max_experiments must be between 1 and 100".to_string());
    }
    if budget.max_runtime_minutes == 0 || budget.max_runtime_minutes > 1_440 {
        return Err("phase 1 budget.max_runtime_minutes must be between 1 and 1440".to_string());
    }
    if budget.max_consecutive_failures == 0 || budget.max_consecutive_failures > 10 {
        return Err("phase 1 budget.max_consecutive_failures must be between 1 and 10".to_string());
    }
    if let Some(max_cost_usd) = budget.max_total_cost_usd {
        if !max_cost_usd.is_finite() || max_cost_usd <= 0.0 {
            return Err("phase 1 budget.max_total_cost_usd must be positive".to_string());
        }
    }
    if let Some(max_total_tokens) = budget.max_total_tokens {
        if max_total_tokens == 0 {
            return Err("phase 1 budget.max_total_tokens must be positive".to_string());
        }
    }
    Ok(())
}

pub fn load_optimization_phase1_config(
    frozen_artifacts: &OptimizationFrozenArtifacts,
) -> Result<OptimizationPhase1Config, String> {
    let objective_markdown =
        read_optimization_artifact_text(&frozen_artifacts.objective, "objective")?;
    if objective_markdown.trim().is_empty() {
        return Err("objective artifact must not be empty".to_string());
    }
    let eval = parse_yaml_artifact::<OptimizationEvalSpec>(&frozen_artifacts.eval, "eval")?;
    let mutation_policy = parse_yaml_artifact::<OptimizationMutationPolicy>(
        &frozen_artifacts.mutation_policy,
        "mutation policy",
    )?;
    let scope = parse_yaml_artifact::<OptimizationSafetyScope>(&frozen_artifacts.scope, "scope")?;
    let budget =
        parse_yaml_artifact::<OptimizationBudgetPolicy>(&frozen_artifacts.budget, "budget")?;
    validate_phase1_eval_spec(&eval)?;
    validate_phase1_mutation_policy(&mutation_policy)?;
    validate_phase1_scope(&scope)?;
    validate_phase1_budget(&budget)?;
    Ok(OptimizationPhase1Config {
        objective_markdown: objective_markdown.trim().to_string(),
        eval,
        mutation_policy,
        scope,
        budget,
    })
}

fn workflow_has_simple_retry_field(
    workflow: &AutomationV2Spec,
    field: OptimizationMutableField,
) -> bool {
    workflow.flow.nodes.iter().any(|node| {
        node.retry_policy
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|obj| match field {
                OptimizationMutableField::RetryPolicyMaxAttempts => obj.get("max_attempts"),
                OptimizationMutableField::RetryPolicyRetries => obj.get("retries"),
                _ => None,
            })
            .and_then(Value::as_i64)
            .is_some()
    })
}

pub fn validate_phase1_workflow_target(
    workflow: &AutomationV2Spec,
    phase1: &OptimizationPhase1Config,
) -> Result<(), String> {
    if workflow.flow.nodes.is_empty() {
        return Err("phase 1 workflow target must contain at least one node".to_string());
    }
    if !workflow.flow.nodes.iter().any(|node| {
        node.output_contract
            .as_ref()
            .and_then(|contract| contract.validator)
            .is_some()
    }) {
        return Err(
            "phase 1 workflow target must contain at least one validator-backed output contract"
                .to_string(),
        );
    }
    let has_objective_field = phase1
        .mutation_policy
        .allowed_text_fields
        .contains(&OptimizationMutableField::Objective);
    let has_summary_guidance_field = phase1
        .mutation_policy
        .allowed_text_fields
        .contains(&OptimizationMutableField::OutputContractSummaryGuidance)
        && workflow.flow.nodes.iter().any(|node| {
            node.output_contract
                .as_ref()
                .and_then(|contract| contract.summary_guidance.as_ref())
                .is_some()
        });
    let has_timeout_field = phase1
        .mutation_policy
        .allowed_knob_fields
        .contains(&OptimizationMutableField::TimeoutMs)
        && workflow
            .flow
            .nodes
            .iter()
            .any(|node| node.timeout_ms.is_some());
    let has_retry_field = phase1
        .mutation_policy
        .allowed_knob_fields
        .iter()
        .copied()
        .filter(|field| {
            matches!(
                field,
                OptimizationMutableField::RetryPolicyMaxAttempts
                    | OptimizationMutableField::RetryPolicyRetries
            )
        })
        .any(|field| workflow_has_simple_retry_field(workflow, field));
    if !(has_objective_field || has_summary_guidance_field || has_timeout_field || has_retry_field)
    {
        return Err(
            "phase 1 workflow target does not expose any mutable fields allowed by the mutation policy"
                .to_string(),
        );
    }
    Ok(())
}

fn mutable_field_label(field: OptimizationMutableField) -> &'static str {
    match field {
        OptimizationMutableField::Objective => "objective",
        OptimizationMutableField::OutputContractSummaryGuidance => {
            "output_contract.summary_guidance"
        }
        OptimizationMutableField::TimeoutMs => "timeout_ms",
        OptimizationMutableField::RetryPolicyMaxAttempts => "retry_policy.max_attempts",
        OptimizationMutableField::RetryPolicyRetries => "retry_policy.retries",
    }
}

fn json_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn normalized_workflow_without_flow(snapshot: &AutomationV2Spec) -> Value {
    let mut value = json_value(snapshot);
    if let Some(obj) = value.as_object_mut() {
        obj.remove("flow");
    }
    value
}

fn normalized_node_static_fields(node: &crate::AutomationFlowNode) -> Value {
    let mut value = json_value(node);
    if let Some(obj) = value.as_object_mut() {
        obj.remove("objective");
        obj.remove("output_contract");
        obj.remove("retry_policy");
        obj.remove("timeout_ms");
    }
    value
}

fn normalized_output_contract(contract: &Option<crate::AutomationFlowOutputContract>) -> Value {
    let mut value = json_value(contract);
    if let Some(obj) = value.as_object_mut() {
        obj.remove("summary_guidance");
    }
    value
}

fn normalized_retry_policy(policy: &Option<Value>) -> Value {
    let mut value = policy.clone().unwrap_or(Value::Null);
    if let Some(obj) = value.as_object_mut() {
        obj.remove("max_attempts");
        obj.remove("retries");
    }
    value
}

fn retry_field_value(policy: &Option<Value>, field: OptimizationMutableField) -> Option<i64> {
    policy
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|obj| match field {
            OptimizationMutableField::RetryPolicyMaxAttempts => obj.get("max_attempts"),
            OptimizationMutableField::RetryPolicyRetries => obj.get("retries"),
            _ => None,
        })
        .and_then(Value::as_i64)
}

fn text_delta_chars(before: &str, after: &str) -> usize {
    let before_chars = before.chars().collect::<Vec<_>>();
    let after_chars = after.chars().collect::<Vec<_>>();
    let mut prefix = 0usize;
    while prefix < before_chars.len()
        && prefix < after_chars.len()
        && before_chars[prefix] == after_chars[prefix]
    {
        prefix += 1;
    }
    let mut before_end = before_chars.len();
    let mut after_end = after_chars.len();
    while before_end > prefix
        && after_end > prefix
        && before_chars[before_end - 1] == after_chars[after_end - 1]
    {
        before_end -= 1;
        after_end -= 1;
    }
    (before_end - prefix) + (after_end - prefix)
}

fn validate_text_mutation(
    node_id: &str,
    field: OptimizationMutableField,
    before: &str,
    after: &str,
    policy: &OptimizationMutationPolicy,
) -> Result<OptimizationValidatedMutation, String> {
    if after.trim().is_empty() {
        return Err(format!(
            "node `{node_id}` {} must not become empty",
            mutable_field_label(field)
        ));
    }
    let delta_chars = text_delta_chars(before, after);
    let baseline_len = before.chars().count().max(1);
    let delta_ratio = delta_chars as f64 / baseline_len as f64;
    if delta_chars == 0 {
        return Err(format!(
            "node `{node_id}` {} must change",
            mutable_field_label(field)
        ));
    }
    if delta_chars > policy.max_text_delta_chars as usize {
        return Err(format!(
            "node `{node_id}` {} exceeds phase 1 max_text_delta_chars",
            mutable_field_label(field)
        ));
    }
    if delta_ratio > policy.max_text_delta_ratio {
        return Err(format!(
            "node `{node_id}` {} exceeds phase 1 max_text_delta_ratio",
            mutable_field_label(field)
        ));
    }
    Ok(OptimizationValidatedMutation {
        node_id: node_id.to_string(),
        field,
        summary: format!(
            "mutate node `{node_id}` {} delta_chars={} delta_ratio={delta_ratio:.3}",
            mutable_field_label(field),
            delta_chars
        ),
    })
}

fn validate_timeout_mutation(
    node_id: &str,
    before: Option<u64>,
    after: Option<u64>,
    policy: &OptimizationMutationPolicy,
) -> Result<OptimizationValidatedMutation, String> {
    let before = before.ok_or_else(|| {
        format!("node `{node_id}` timeout_ms is not mutable in phase 1 because it is absent")
    })?;
    let after = after
        .ok_or_else(|| format!("node `{node_id}` timeout_ms may not be removed in phase 1"))?;
    if after < policy.timeout_min_ms || after > policy.timeout_max_ms {
        return Err(format!(
            "node `{node_id}` timeout_ms must stay within {}..={} ms",
            policy.timeout_min_ms, policy.timeout_max_ms
        ));
    }
    let delta = after.abs_diff(before);
    let allowed_percent_delta = ((before as f64) * policy.timeout_delta_percent).ceil() as u64;
    if delta == 0 {
        return Err(format!("node `{node_id}` timeout_ms must change"));
    }
    if delta > policy.timeout_delta_ms || delta > allowed_percent_delta {
        return Err(format!(
            "node `{node_id}` timeout_ms exceeds phase 1 timeout delta limits"
        ));
    }
    Ok(OptimizationValidatedMutation {
        node_id: node_id.to_string(),
        field: OptimizationMutableField::TimeoutMs,
        summary: format!(
            "mutate node `{node_id}` timeout_ms from {before} to {after} (delta={delta})"
        ),
    })
}

fn validate_retry_mutation(
    node_id: &str,
    field: OptimizationMutableField,
    before: Option<i64>,
    after: Option<i64>,
    policy: &OptimizationMutationPolicy,
) -> Result<OptimizationValidatedMutation, String> {
    let before = before.ok_or_else(|| {
        format!(
            "node `{node_id}` {} is not mutable in phase 1 because it is absent or non-integer",
            mutable_field_label(field)
        )
    })?;
    let after = after.ok_or_else(|| {
        format!(
            "node `{node_id}` {} may not be removed in phase 1",
            mutable_field_label(field)
        )
    })?;
    if after < policy.retry_min as i64 || after > policy.retry_max as i64 {
        return Err(format!(
            "node `{node_id}` {} must stay within {}..={}",
            mutable_field_label(field),
            policy.retry_min,
            policy.retry_max
        ));
    }
    let delta = (after - before).abs();
    if delta == 0 {
        return Err(format!(
            "node `{node_id}` {} must change",
            mutable_field_label(field)
        ));
    }
    if delta > policy.retry_delta.abs() as i64 {
        return Err(format!(
            "node `{node_id}` {} exceeds phase 1 retry delta limit",
            mutable_field_label(field)
        ));
    }
    Ok(OptimizationValidatedMutation {
        node_id: node_id.to_string(),
        field,
        summary: format!(
            "mutate node `{node_id}` {} from {before} to {after}",
            mutable_field_label(field)
        ),
    })
}

pub fn validate_phase1_candidate_mutation(
    baseline: &AutomationV2Spec,
    candidate: &AutomationV2Spec,
    phase1: &OptimizationPhase1Config,
) -> Result<OptimizationValidatedMutation, String> {
    if normalized_workflow_without_flow(baseline) != normalized_workflow_without_flow(candidate) {
        return Err(
            "phase 1 candidate may not mutate workflow fields outside flow.nodes".to_string(),
        );
    }
    if baseline.flow.nodes.len() != candidate.flow.nodes.len() {
        return Err("phase 1 candidate may not add or remove workflow nodes".to_string());
    }
    let mut changes = Vec::new();
    for (baseline_node, candidate_node) in
        baseline.flow.nodes.iter().zip(candidate.flow.nodes.iter())
    {
        if baseline_node.node_id != candidate_node.node_id {
            return Err("phase 1 candidate may not reorder or replace workflow nodes".to_string());
        }
        if normalized_node_static_fields(baseline_node)
            != normalized_node_static_fields(candidate_node)
        {
            return Err(format!(
                "phase 1 candidate may not mutate node `{}` outside the allowed field families",
                baseline_node.node_id
            ));
        }
        if normalized_output_contract(&baseline_node.output_contract)
            != normalized_output_contract(&candidate_node.output_contract)
        {
            return Err(format!(
                "phase 1 candidate may not mutate node `{}` output_contract outside summary_guidance",
                baseline_node.node_id
            ));
        }
        if normalized_retry_policy(&baseline_node.retry_policy)
            != normalized_retry_policy(&candidate_node.retry_policy)
        {
            return Err(format!(
                "phase 1 candidate may not mutate node `{}` retry_policy outside max_attempts/retries",
                baseline_node.node_id
            ));
        }
        if baseline_node.objective != candidate_node.objective {
            if !phase1
                .mutation_policy
                .allowed_text_fields
                .contains(&OptimizationMutableField::Objective)
            {
                return Err(format!(
                    "node `{}` objective is not allowed by the phase 1 mutation policy",
                    baseline_node.node_id
                ));
            }
            changes.push(validate_text_mutation(
                &baseline_node.node_id,
                OptimizationMutableField::Objective,
                &baseline_node.objective,
                &candidate_node.objective,
                &phase1.mutation_policy,
            )?);
        }
        let baseline_summary = baseline_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.summary_guidance.as_deref());
        let candidate_summary = candidate_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.summary_guidance.as_deref());
        if baseline_summary != candidate_summary {
            if !phase1
                .mutation_policy
                .allowed_text_fields
                .contains(&OptimizationMutableField::OutputContractSummaryGuidance)
            {
                return Err(format!(
                    "node `{}` output_contract.summary_guidance is not allowed by the phase 1 mutation policy",
                    baseline_node.node_id
                ));
            }
            let before = baseline_summary.ok_or_else(|| {
                format!(
                    "node `{}` output_contract.summary_guidance may not be created in phase 1",
                    baseline_node.node_id
                )
            })?;
            let after = candidate_summary.ok_or_else(|| {
                format!(
                    "node `{}` output_contract.summary_guidance may not be removed in phase 1",
                    baseline_node.node_id
                )
            })?;
            changes.push(validate_text_mutation(
                &baseline_node.node_id,
                OptimizationMutableField::OutputContractSummaryGuidance,
                before,
                after,
                &phase1.mutation_policy,
            )?);
        }
        if baseline_node.timeout_ms != candidate_node.timeout_ms {
            if !phase1
                .mutation_policy
                .allowed_knob_fields
                .contains(&OptimizationMutableField::TimeoutMs)
            {
                return Err(format!(
                    "node `{}` timeout_ms is not allowed by the phase 1 mutation policy",
                    baseline_node.node_id
                ));
            }
            changes.push(validate_timeout_mutation(
                &baseline_node.node_id,
                baseline_node.timeout_ms,
                candidate_node.timeout_ms,
                &phase1.mutation_policy,
            )?);
        }
        for field in [
            OptimizationMutableField::RetryPolicyMaxAttempts,
            OptimizationMutableField::RetryPolicyRetries,
        ] {
            let before = retry_field_value(&baseline_node.retry_policy, field);
            let after = retry_field_value(&candidate_node.retry_policy, field);
            if before != after {
                if !phase1.mutation_policy.allowed_knob_fields.contains(&field) {
                    return Err(format!(
                        "node `{}` {} is not allowed by the phase 1 mutation policy",
                        baseline_node.node_id,
                        mutable_field_label(field)
                    ));
                }
                changes.push(validate_retry_mutation(
                    &baseline_node.node_id,
                    field,
                    before,
                    after,
                    &phase1.mutation_policy,
                )?);
            }
        }
    }
    if changes.is_empty() {
        return Err("phase 1 candidate must change exactly one allowed field family".to_string());
    }
    let changed_nodes = changes
        .iter()
        .map(|change| change.node_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if changed_nodes.len() > phase1.mutation_policy.max_nodes_changed_per_candidate as usize {
        return Err("phase 1 candidate may only change one node per experiment".to_string());
    }
    if changes.len()
        > phase1
            .mutation_policy
            .max_field_families_changed_per_candidate as usize
    {
        return Err(
            "phase 1 candidate may only change one field family per experiment".to_string(),
        );
    }
    Ok(changes.into_iter().next().expect("non-empty change set"))
}

fn metric_f64(metrics: &Value, key: &str) -> Option<f64> {
    metrics.get(key).and_then(Value::as_f64)
}

pub fn parse_phase1_metrics(metrics: &Value) -> Result<OptimizationPhase1Metrics, String> {
    let artifact_validator_pass_rate = metric_f64(metrics, "artifact_validator_pass_rate")
        .or_else(|| metric_f64(metrics, "validator_pass_rate"))
        .ok_or_else(|| "phase 1 metrics require artifact_validator_pass_rate".to_string())?;
    let unmet_requirement_count = metric_f64(metrics, "unmet_requirement_count")
        .ok_or_else(|| "phase 1 metrics require unmet_requirement_count".to_string())?;
    let blocked_node_rate = metric_f64(metrics, "blocked_node_rate")
        .ok_or_else(|| "phase 1 metrics require blocked_node_rate".to_string())?;
    let budget_within_limits = metrics
        .get("budget_within_limits")
        .and_then(Value::as_bool)
        .ok_or_else(|| "phase 1 metrics require budget_within_limits".to_string())?;
    Ok(OptimizationPhase1Metrics {
        artifact_validator_pass_rate,
        unmet_requirement_count,
        blocked_node_rate,
        budget_within_limits,
    })
}

pub fn derive_phase1_metrics_from_run(
    run: &AutomationV2RunRecord,
    baseline_snapshot: &AutomationV2Spec,
    phase1: &OptimizationPhase1Config,
) -> Result<OptimizationPhase1Metrics, String> {
    let total_nodes = baseline_snapshot.flow.nodes.len().max(1) as f64;
    let validator_outputs = run
        .checkpoint
        .node_outputs
        .values()
        .filter_map(Value::as_object)
        .filter_map(|row| row.get("validator_summary"))
        .filter_map(Value::as_object)
        .collect::<Vec<_>>();
    if validator_outputs.is_empty() {
        return Err("automation run does not contain validator-backed outputs".to_string());
    }
    let passed_count = validator_outputs
        .iter()
        .filter(|summary| {
            summary
                .get("outcome")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case("passed"))
        })
        .count() as f64;
    let unmet_requirement_count = validator_outputs
        .iter()
        .map(|summary| {
            summary
                .get("unmet_requirements")
                .and_then(Value::as_array)
                .map(|rows| rows.len() as f64)
                .unwrap_or(0.0)
        })
        .sum::<f64>();
    let runtime_ms = run
        .finished_at_ms
        .or(Some(run.updated_at_ms))
        .unwrap_or(run.updated_at_ms)
        .saturating_sub(run.started_at_ms.unwrap_or(run.created_at_ms));
    let within_tokens = phase1
        .budget
        .max_total_tokens
        .is_none_or(|limit| run.total_tokens <= limit);
    let within_cost = phase1
        .budget
        .max_total_cost_usd
        .is_none_or(|limit| run.estimated_cost_usd <= limit);
    let within_runtime =
        runtime_ms <= (phase1.budget.max_runtime_minutes as u64).saturating_mul(60_000);
    Ok(OptimizationPhase1Metrics {
        artifact_validator_pass_rate: passed_count / validator_outputs.len() as f64,
        unmet_requirement_count,
        blocked_node_rate: run.checkpoint.blocked_nodes.len() as f64 / total_nodes,
        budget_within_limits: within_tokens && within_cost && within_runtime,
    })
}

pub fn derive_phase1_validator_case_outcomes_from_run(
    run: &AutomationV2RunRecord,
) -> std::collections::BTreeMap<String, String> {
    run.checkpoint
        .node_outputs
        .iter()
        .filter_map(|(node_id, output)| {
            let outcome = output
                .as_object()
                .and_then(|row| row.get("validator_summary"))
                .and_then(Value::as_object)
                .and_then(|summary| summary.get("outcome"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            Some((node_id.clone(), outcome.to_ascii_lowercase()))
        })
        .collect()
}

pub fn evaluate_phase1_promotion(
    baseline: &OptimizationPhase1Metrics,
    candidate: &OptimizationPhase1Metrics,
) -> OptimizationPromotionDecision {
    if !candidate.budget_within_limits {
        return OptimizationPromotionDecision {
            decision: OptimizationPromotionDecisionKind::Discard,
            reason: "candidate exceeded phase 1 budget ceilings".to_string(),
        };
    }
    if candidate.blocked_node_rate > baseline.blocked_node_rate {
        return OptimizationPromotionDecision {
            decision: OptimizationPromotionDecisionKind::Discard,
            reason: "candidate increased blocked_node_rate".to_string(),
        };
    }
    if candidate.artifact_validator_pass_rate > baseline.artifact_validator_pass_rate {
        return OptimizationPromotionDecision {
            decision: OptimizationPromotionDecisionKind::Promote,
            reason: "candidate improved artifact_validator_pass_rate".to_string(),
        };
    }
    if (candidate.artifact_validator_pass_rate - baseline.artifact_validator_pass_rate).abs()
        <= f64::EPSILON
        && candidate.unmet_requirement_count < baseline.unmet_requirement_count
    {
        return OptimizationPromotionDecision {
            decision: OptimizationPromotionDecisionKind::Promote,
            reason: "candidate improved unmet_requirement_count on a primary-metric tie"
                .to_string(),
        };
    }
    OptimizationPromotionDecision {
        decision: OptimizationPromotionDecisionKind::Discard,
        reason: "candidate did not beat the current phase 1 baseline".to_string(),
    }
}

pub fn establish_phase1_baseline(
    replays: &[OptimizationBaselineReplayRecord],
    phase1: &OptimizationPhase1Config,
) -> Result<OptimizationPhase1Metrics, String> {
    let required_runs = phase1.eval.campaign_start_baseline_runs.max(1) as usize;
    if replays.len() < required_runs {
        return Err(format!(
            "phase 1 baseline establishment requires at least {required_runs} replay runs"
        ));
    }
    let relevant = &replays[replays.len() - required_runs..];
    if relevant
        .iter()
        .any(|replay| !replay.phase1_metrics.budget_within_limits)
    {
        return Err("phase 1 baseline replay exceeded budget ceilings".to_string());
    }
    let validator_min = relevant
        .iter()
        .map(|replay| replay.phase1_metrics.artifact_validator_pass_rate)
        .fold(f64::INFINITY, f64::min);
    let validator_max = relevant
        .iter()
        .map(|replay| replay.phase1_metrics.artifact_validator_pass_rate)
        .fold(f64::NEG_INFINITY, f64::max);
    if validator_max - validator_min > 0.05 {
        return Err(
            "phase 1 baseline replay drift exceeded 5 percentage points for artifact_validator_pass_rate"
                .to_string(),
        );
    }
    let blocked_min = relevant
        .iter()
        .map(|replay| replay.phase1_metrics.blocked_node_rate)
        .fold(f64::INFINITY, f64::min);
    let blocked_max = relevant
        .iter()
        .map(|replay| replay.phase1_metrics.blocked_node_rate)
        .fold(f64::NEG_INFINITY, f64::max);
    if blocked_max - blocked_min > 0.05 {
        return Err(
            "phase 1 baseline replay drift exceeded 5 percentage points for blocked_node_rate"
                .to_string(),
        );
    }
    let mut case_outcomes =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for replay in relevant {
        for (case_id, outcome) in &replay.validator_case_outcomes {
            case_outcomes
                .entry(case_id.clone())
                .or_default()
                .insert(outcome.clone());
        }
    }
    let total_cases = case_outcomes.len();
    if total_cases > 0 {
        let flaky_cases = case_outcomes
            .values()
            .filter(|outcomes| outcomes.len() > 1)
            .count();
        if (flaky_cases as f64 / total_cases as f64) > 0.10 {
            return Err(format!(
                "phase 1 baseline replay flakiness exceeded 10% of validator cases ({flaky_cases}/{total_cases})"
            ));
        }
    }
    let count = relevant.len() as f64;
    Ok(OptimizationPhase1Metrics {
        artifact_validator_pass_rate: relevant
            .iter()
            .map(|replay| replay.phase1_metrics.artifact_validator_pass_rate)
            .sum::<f64>()
            / count,
        unmet_requirement_count: relevant
            .iter()
            .map(|replay| replay.phase1_metrics.unmet_requirement_count)
            .sum::<f64>()
            / count,
        blocked_node_rate: relevant
            .iter()
            .map(|replay| replay.phase1_metrics.blocked_node_rate)
            .sum::<f64>()
            / count,
        budget_within_limits: true,
    })
}

pub fn phase1_baseline_replay_due(
    replays: &[OptimizationBaselineReplayRecord],
    pending_run_count: usize,
    phase1: &OptimizationPhase1Config,
    experiment_count: usize,
    now_ms: u64,
) -> bool {
    if pending_run_count > 0 {
        return false;
    }
    let required_runs = phase1.eval.campaign_start_baseline_runs.max(1) as usize;
    if replays.len() < required_runs {
        return true;
    }
    let Some(last_replay) = replays.last() else {
        return true;
    };
    let candidate_interval = phase1.eval.baseline_replay_every_candidates.max(1) as usize;
    let candidates_since_last =
        experiment_count.saturating_sub(last_replay.experiment_count_at_recording as usize);
    if candidates_since_last >= candidate_interval {
        return true;
    }
    let replay_interval_ms = (phase1.eval.baseline_replay_every_minutes.max(1) as u64) * 60_000;
    now_ms.saturating_sub(last_replay.recorded_at_ms) >= replay_interval_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_phase1() -> OptimizationPhase1Config {
        OptimizationPhase1Config {
            objective_markdown: "improve output quality".to_string(),
            eval: OptimizationEvalSpec {
                pack_ref: "eval-pack.jsonl".to_string(),
                primary_metric: OptimizationMetricKind::ArtifactValidatorPassRate,
                secondary_metric: OptimizationMetricKind::UnmetRequirementCount,
                hard_guardrails: vec![
                    OptimizationGuardrailKind::BlockedNodeRate,
                    OptimizationGuardrailKind::BudgetCeilings,
                ],
                campaign_start_baseline_runs: 2,
                baseline_replay_every_candidates: 5,
                baseline_replay_every_minutes: 30,
            },
            mutation_policy: OptimizationMutationPolicy {
                max_nodes_changed_per_candidate: 1,
                max_field_families_changed_per_candidate: 1,
                allowed_text_fields: vec![
                    OptimizationMutableField::Objective,
                    OptimizationMutableField::OutputContractSummaryGuidance,
                ],
                allowed_knob_fields: vec![
                    OptimizationMutableField::TimeoutMs,
                    OptimizationMutableField::RetryPolicyMaxAttempts,
                    OptimizationMutableField::RetryPolicyRetries,
                ],
                max_text_delta_chars: 300,
                max_text_delta_ratio: 0.25,
                timeout_delta_percent: 0.15,
                timeout_delta_ms: 30_000,
                timeout_min_ms: 30_000,
                timeout_max_ms: 600_000,
                retry_delta: 1,
                retry_min: 0,
                retry_max: 3,
                allow_text_and_knob_bundle: false,
            },
            scope: OptimizationSafetyScope {
                candidate_snapshot_only: true,
                allow_live_source_mutation: false,
                allow_external_side_effects_in_eval: false,
                promotion_requires_operator_approval: true,
                forbidden_fields: vec!["agents".to_string()],
            },
            budget: OptimizationBudgetPolicy {
                max_experiments: 10,
                max_runtime_minutes: 60,
                max_consecutive_failures: 3,
                max_total_tokens: Some(50_000),
                max_total_cost_usd: Some(10.0),
            },
        }
    }

    fn sample_workflow() -> AutomationV2Spec {
        AutomationV2Spec {
            automation_id: "wf-opt".to_string(),
            name: "Optimization Target".to_string(),
            description: Some("workflow".to_string()),
            status: crate::AutomationV2Status::Draft,
            schedule: crate::AutomationV2Schedule {
                schedule_type: crate::AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::Skip,
            },
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            agents: vec![crate::AutomationAgentProfile {
                agent_id: "agent-1".to_string(),
                template_id: None,
                display_name: "Worker".to_string(),
                avatar_url: None,
                model_policy: None,
                skills: Vec::new(),
                tool_policy: crate::AutomationAgentToolPolicy {
                    allowlist: Vec::new(),
                    denylist: Vec::new(),
                },
                mcp_policy: crate::AutomationAgentMcpPolicy {
                    allowed_servers: Vec::new(),
                    allowed_tools: None,
                },
                approval_policy: None,
            }],
            flow: crate::AutomationFlowSpec {
                nodes: vec![crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "node-1".to_string(),
                    agent_id: "agent-1".to_string(),
                    objective: "Write a concise report for the user".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: Some(crate::AutomationFlowOutputContract {
                        kind: "report".to_string(),
                        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
                        enforcement: None,
                        schema: None,
                        summary_guidance: Some("Summarize clearly.".to_string()),
                    }),
                    retry_policy: Some(json!({ "max_attempts": 1, "retries": 0 })),
                    timeout_ms: Some(60_000),
                    max_tool_calls: None,
                    stage_kind: None,
                    gate: None,
                    metadata: None,
                }],
            },
            execution: crate::AutomationExecutionPolicy {
                max_parallel_agents: None,
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: "test".to_string(),
            workspace_root: Some("/tmp/workflow".to_string()),
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        }
    }

    #[test]
    fn validate_phase1_candidate_accepts_single_objective_change() {
        let phase1 = sample_phase1();
        let baseline = sample_workflow();
        let mut candidate = baseline.clone();
        candidate.flow.nodes[0].objective = "Write a concise report for the team".to_string();
        let mutation =
            validate_phase1_candidate_mutation(&baseline, &candidate, &phase1).expect("valid");
        assert_eq!(mutation.node_id, "node-1");
        assert_eq!(mutation.field, OptimizationMutableField::Objective);
    }

    #[test]
    fn validate_phase1_candidate_rejects_mutation_bundle() {
        let phase1 = sample_phase1();
        let baseline = sample_workflow();
        let mut candidate = baseline.clone();
        candidate.flow.nodes[0].objective = "Write a concise report for the team".to_string();
        candidate.flow.nodes[0].timeout_ms = Some(65_000);
        let error =
            validate_phase1_candidate_mutation(&baseline, &candidate, &phase1).expect_err("bundle");
        assert!(error.contains("one field family"));
    }

    #[test]
    fn validate_phase1_candidate_rejects_oversize_text_delta() {
        let phase1 = sample_phase1();
        let baseline = sample_workflow();
        let mut candidate = baseline.clone();
        candidate.flow.nodes[0].objective = "x".repeat(400);
        let error = validate_phase1_candidate_mutation(&baseline, &candidate, &phase1)
            .expect_err("oversize");
        assert!(error.contains("max_text_delta_chars") || error.contains("max_text_delta_ratio"));
    }

    #[test]
    fn evaluate_phase1_promotion_prefers_primary_metric() {
        let baseline = OptimizationPhase1Metrics {
            artifact_validator_pass_rate: 0.7,
            unmet_requirement_count: 2.0,
            blocked_node_rate: 0.1,
            budget_within_limits: true,
        };
        let candidate = OptimizationPhase1Metrics {
            artifact_validator_pass_rate: 0.8,
            unmet_requirement_count: 3.0,
            blocked_node_rate: 0.1,
            budget_within_limits: true,
        };
        let decision = evaluate_phase1_promotion(&baseline, &candidate);
        assert_eq!(
            decision.decision,
            OptimizationPromotionDecisionKind::Promote
        );
    }

    #[test]
    fn evaluate_phase1_promotion_uses_secondary_metric_on_tie() {
        let baseline = OptimizationPhase1Metrics {
            artifact_validator_pass_rate: 0.8,
            unmet_requirement_count: 2.0,
            blocked_node_rate: 0.1,
            budget_within_limits: true,
        };
        let candidate = OptimizationPhase1Metrics {
            artifact_validator_pass_rate: 0.8,
            unmet_requirement_count: 1.0,
            blocked_node_rate: 0.1,
            budget_within_limits: true,
        };
        let decision = evaluate_phase1_promotion(&baseline, &candidate);
        assert_eq!(
            decision.decision,
            OptimizationPromotionDecisionKind::Promote
        );
    }

    #[test]
    fn evaluate_phase1_promotion_rejects_guardrail_regression() {
        let baseline = OptimizationPhase1Metrics {
            artifact_validator_pass_rate: 0.8,
            unmet_requirement_count: 2.0,
            blocked_node_rate: 0.1,
            budget_within_limits: true,
        };
        let candidate = OptimizationPhase1Metrics {
            artifact_validator_pass_rate: 0.9,
            unmet_requirement_count: 1.0,
            blocked_node_rate: 0.2,
            budget_within_limits: true,
        };
        let decision = evaluate_phase1_promotion(&baseline, &candidate);
        assert_eq!(
            decision.decision,
            OptimizationPromotionDecisionKind::Discard
        );
    }

    #[test]
    fn establish_phase1_baseline_averages_stable_replays() {
        let phase1 = sample_phase1();
        let replays = vec![
            OptimizationBaselineReplayRecord {
                replay_id: "replay-1".to_string(),
                automation_run_id: None,
                phase1_metrics: OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 0.8,
                    unmet_requirement_count: 1.0,
                    blocked_node_rate: 0.0,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([(
                    "node-1".to_string(),
                    "passed".to_string(),
                )]),
                experiment_count_at_recording: 0,
                recorded_at_ms: 1,
            },
            OptimizationBaselineReplayRecord {
                replay_id: "replay-2".to_string(),
                automation_run_id: None,
                phase1_metrics: OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 0.84,
                    unmet_requirement_count: 2.0,
                    blocked_node_rate: 0.02,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([(
                    "node-1".to_string(),
                    "passed".to_string(),
                )]),
                experiment_count_at_recording: 0,
                recorded_at_ms: 2,
            },
        ];
        let baseline = establish_phase1_baseline(&replays, &phase1).expect("stable");
        assert!((baseline.artifact_validator_pass_rate - 0.82).abs() < 1e-9);
        assert!((baseline.unmet_requirement_count - 1.5).abs() < 1e-9);
        assert!((baseline.blocked_node_rate - 0.01).abs() < 1e-9);
    }

    #[test]
    fn establish_phase1_baseline_rejects_validator_drift() {
        let phase1 = sample_phase1();
        let replays = vec![
            OptimizationBaselineReplayRecord {
                replay_id: "replay-1".to_string(),
                automation_run_id: None,
                phase1_metrics: OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 0.8,
                    unmet_requirement_count: 1.0,
                    blocked_node_rate: 0.0,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([(
                    "node-1".to_string(),
                    "passed".to_string(),
                )]),
                experiment_count_at_recording: 0,
                recorded_at_ms: 1,
            },
            OptimizationBaselineReplayRecord {
                replay_id: "replay-2".to_string(),
                automation_run_id: None,
                phase1_metrics: OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 0.9,
                    unmet_requirement_count: 1.0,
                    blocked_node_rate: 0.0,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([(
                    "node-1".to_string(),
                    "passed".to_string(),
                )]),
                experiment_count_at_recording: 0,
                recorded_at_ms: 2,
            },
        ];
        let error = establish_phase1_baseline(&replays, &phase1).expect_err("drift");
        assert!(error.contains("artifact_validator_pass_rate"));
    }

    #[test]
    fn establish_phase1_baseline_rejects_flaky_validator_cases() {
        let phase1 = sample_phase1();
        let replays = vec![
            OptimizationBaselineReplayRecord {
                replay_id: "replay-1".to_string(),
                automation_run_id: None,
                phase1_metrics: OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 1.0,
                    unmet_requirement_count: 0.0,
                    blocked_node_rate: 0.0,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([
                    ("node-1".to_string(), "passed".to_string()),
                    ("node-2".to_string(), "passed".to_string()),
                    ("node-3".to_string(), "passed".to_string()),
                    ("node-4".to_string(), "passed".to_string()),
                    ("node-5".to_string(), "passed".to_string()),
                    ("node-6".to_string(), "passed".to_string()),
                    ("node-7".to_string(), "passed".to_string()),
                    ("node-8".to_string(), "passed".to_string()),
                ]),
                experiment_count_at_recording: 0,
                recorded_at_ms: 1,
            },
            OptimizationBaselineReplayRecord {
                replay_id: "replay-2".to_string(),
                automation_run_id: None,
                phase1_metrics: OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 1.0,
                    unmet_requirement_count: 0.0,
                    blocked_node_rate: 0.0,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([
                    ("node-1".to_string(), "blocked".to_string()),
                    ("node-2".to_string(), "blocked".to_string()),
                    ("node-3".to_string(), "passed".to_string()),
                    ("node-4".to_string(), "passed".to_string()),
                    ("node-5".to_string(), "passed".to_string()),
                    ("node-6".to_string(), "passed".to_string()),
                    ("node-7".to_string(), "passed".to_string()),
                    ("node-8".to_string(), "passed".to_string()),
                ]),
                experiment_count_at_recording: 0,
                recorded_at_ms: 2,
            },
        ];
        let error = establish_phase1_baseline(&replays, &phase1).expect_err("flaky");
        assert!(error.contains("flakiness"));
    }

    #[test]
    fn derive_phase1_metrics_from_run_uses_validator_outputs_and_budget() {
        let phase1 = sample_phase1();
        let workflow = sample_workflow();
        let run = AutomationV2RunRecord {
            run_id: "run-1".to_string(),
            automation_id: workflow.automation_id.clone(),
            tenant_context: tandem_types::TenantContext::local_implicit(),
            trigger_type: "manual".to_string(),
            status: crate::AutomationRunStatus::Completed,
            created_at_ms: 1,
            updated_at_ms: 10_000,
            started_at_ms: Some(1_000),
            finished_at_ms: Some(9_000),
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: crate::AutomationRunCheckpoint {
                completed_nodes: vec!["node-1".to_string()],
                pending_nodes: Vec::new(),
                node_outputs: std::collections::HashMap::from([
                    (
                        "node-1".to_string(),
                        json!({
                            "validator_summary": {
                                "outcome": "passed",
                                "unmet_requirements": []
                            }
                        }),
                    ),
                    (
                        "node-2".to_string(),
                        json!({
                            "validator_summary": {
                                "outcome": "blocked",
                                "unmet_requirements": ["citation_missing", "web_sources_reviewed_missing"]
                            }
                        }),
                    ),
                ]),
                node_attempts: std::collections::HashMap::new(),
                blocked_nodes: vec!["node-2".to_string()],
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: Some(workflow.clone()),
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 100,
            estimated_cost_usd: 0.5,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
        };
        let metrics = derive_phase1_metrics_from_run(&run, &workflow, &phase1).expect("metrics");
        assert!((metrics.artifact_validator_pass_rate - 0.5).abs() < 1e-9);
        assert!((metrics.unmet_requirement_count - 2.0).abs() < 1e-9);
        assert!((metrics.blocked_node_rate - 1.0).abs() < 1e-9);
        assert!(metrics.budget_within_limits);
        let case_outcomes = derive_phase1_validator_case_outcomes_from_run(&run);
        assert_eq!(
            case_outcomes.get("node-1").map(String::as_str),
            Some("passed")
        );
        assert_eq!(
            case_outcomes.get("node-2").map(String::as_str),
            Some("blocked")
        );
    }

    #[test]
    fn phase1_baseline_replay_due_requires_initial_replays() {
        let phase1 = sample_phase1();
        assert!(phase1_baseline_replay_due(&[], 0, &phase1, 0, 0));
        assert!(!phase1_baseline_replay_due(&[], 1, &phase1, 0, 0));
    }

    #[test]
    fn phase1_baseline_replay_due_uses_candidate_and_time_intervals() {
        let phase1 = sample_phase1();
        let replays = vec![
            OptimizationBaselineReplayRecord {
                replay_id: "replay-1".to_string(),
                automation_run_id: None,
                phase1_metrics: OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 1.0,
                    unmet_requirement_count: 0.0,
                    blocked_node_rate: 0.0,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([(
                    "node-1".to_string(),
                    "passed".to_string(),
                )]),
                experiment_count_at_recording: 2,
                recorded_at_ms: 1_000,
            },
            OptimizationBaselineReplayRecord {
                replay_id: "replay-2".to_string(),
                automation_run_id: None,
                phase1_metrics: OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 1.0,
                    unmet_requirement_count: 0.0,
                    blocked_node_rate: 0.0,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([(
                    "node-1".to_string(),
                    "passed".to_string(),
                )]),
                experiment_count_at_recording: 2,
                recorded_at_ms: 1_500,
            },
        ];
        assert!(phase1_baseline_replay_due(&replays, 0, &phase1, 7, 2_000));
        assert!(phase1_baseline_replay_due(
            &replays, 0, &phase1, 3, 1_801_500
        ));
        assert!(!phase1_baseline_replay_due(&replays, 0, &phase1, 3, 2_000));
    }
}
