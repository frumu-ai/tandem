// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tandem_workflows::plan_package::{AutomationV2ScheduleType, WorkflowPlanStep};

use crate::plan_validation::validate_plan_package;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanLifecycleState {
    Draft,
    Preview,
    AwaitingApproval,
    Approved,
    Applied,
    Active,
    Degraded,
    Paused,
    Superseded,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineSemanticKind {
    Research,
    Monitoring,
    Drafting,
    Review,
    Execution,
    Sync,
    Reporting,
    Publication,
    Remediation,
    Triage,
    Orchestration,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Scheduled,
    Manual,
    EventDriven,
    ApprovalTriggered,
    ReleaseTriggered,
    ArtifactTriggered,
    DependencyTriggered,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    InternalOnly,
    DraftOnly,
    ApprovalRequired,
    AutoApproved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyMode {
    Hard,
    Soft,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyResolutionStrategy {
    TopologicalSequential,
    TopologicalParallel,
    StrictSequential,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PartialFailureMode {
    ContinueIndependent,
    PauseDownstreamOnly,
    PauseAll,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReentryPoint {
    FailedStep,
    RoutineStart,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MidRoutineConnectorFailureMode {
    SurfaceAndPause,
    SurfaceAndDegrade,
    SurfaceAndBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrossRoutineVisibility {
    None,
    DeclaredOutputsOnly,
    PlanOwnerOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionContextScope {
    GoalOnly,
    GoalAndOwnRoutine,
    GoalAndDependencies,
    FullPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunHistoryVisibility {
    RoutineOnly,
    PlanOwner,
    NamedRoles,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntermediateArtifactVisibility {
    RoutineOnly,
    PlanOwner,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinalArtifactVisibility {
    RoutineOnly,
    DeclaredConsumers,
    PlanOwner,
    Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommunicationModel {
    ArtifactOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerVisibility {
    None,
    GoalOnly,
    DeclaredOutputsOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Fast,
    Mid,
    Strong,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextObjectScope {
    Mission,
    Plan,
    Routine,
    Step,
    Handoff,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextValidationStatus {
    Pending,
    Valid,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrecedenceSourceTier {
    CompilerDefault,
    UserOverride,
    ApprovedPlanState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanDiffChangeType {
    Add,
    Update,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManualTriggerSource {
    Calendar,
    Mission,
    Routine,
    Api,
    DryRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanOwner {
    pub owner_id: String,
    pub scope: String,
    pub audience: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionDefinition {
    pub goal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SuccessCriteria {
    #[serde(default)]
    pub required_artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_viable_completion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_window_hours: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextObjectProvenance {
    pub plan_id: String,
    pub routine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextObject {
    pub context_object_id: String,
    pub name: String,
    pub kind: String,
    pub scope: ContextObjectScope,
    pub owner_routine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_step_id: Option<String>,
    #[serde(default)]
    pub declared_consumers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_ref: Option<String>,
    #[serde(default)]
    pub data_scope_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_window_hours: Option<u32>,
    pub validation_status: ContextValidationStatus,
    pub provenance: ContextObjectProvenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrecedenceLogEntry {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_override: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_plan_state: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_value: Option<Value>,
    pub source_tier: PrecedenceSourceTier,
    pub conflict_detected: bool,
    pub resolution_rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanDiffChangedField {
    pub path: String,
    pub change_type: PlanDiffChangeType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_value: Option<Value>,
    pub requires_revalidation: bool,
    pub requires_reapproval: bool,
    pub breaking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlanDiffSummary {
    #[serde(default)]
    pub changed_count: usize,
    #[serde(default)]
    pub breaking_count: usize,
    #[serde(default)]
    pub revalidation_required: bool,
    #[serde(default)]
    pub reapproval_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanDiff {
    pub from_revision: u32,
    pub to_revision: u32,
    #[serde(default)]
    pub changed_fields: Vec<PlanDiffChangedField>,
    pub summary: PlanDiffSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManualTriggerRecord {
    pub trigger_id: String,
    pub plan_id: String,
    pub plan_revision: u32,
    pub routine_id: String,
    pub triggered_by: String,
    pub trigger_source: ManualTriggerSource,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy_snapshot: Option<ApprovalMatrix>,
    #[serde(default)]
    pub connector_binding_snapshot: Vec<ConnectorBinding>,
    pub triggered_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default)]
    pub artifacts_produced: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriggerDefinition {
    #[serde(rename = "type")]
    pub trigger_type: TriggerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutineDependency {
    #[serde(rename = "type")]
    pub dependency_type: String,
    pub routine_id: String,
    pub mode: DependencyMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyResolution {
    pub strategy: DependencyResolutionStrategy,
    pub partial_failure_mode: PartialFailureMode,
    pub reentry_point: ReentryPoint,
    pub mid_routine_connector_failure: MidRoutineConnectorFailureMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RoutineConnectorResolution {
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub binding_options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataScope {
    #[serde(default)]
    pub readable_paths: Vec<String>,
    #[serde(default)]
    pub writable_paths: Vec<String>,
    #[serde(default)]
    pub denied_paths: Vec<String>,
    pub cross_routine_visibility: CrossRoutineVisibility,
    pub mission_context_scope: MissionContextScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_context_justification: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditScope {
    pub run_history_visibility: RunHistoryVisibility,
    #[serde(default)]
    pub named_audit_roles: Vec<String>,
    pub intermediate_artifact_visibility: IntermediateArtifactVisibility,
    pub final_artifact_visibility: FinalArtifactVisibility,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectorRequirement {
    pub capability: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepModelSelection {
    pub tier: ModelTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StepModelPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<StepModelSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelRoutingEntry {
    pub step_id: String,
    pub tier: ModelTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub resolved: bool,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ModelRoutingReport {
    #[serde(default)]
    pub tier_assigned_count: usize,
    #[serde(default)]
    pub provider_unresolved_count: usize,
    #[serde(default)]
    pub entries: Vec<ModelRoutingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuccessCriteriaSubjectKind {
    Plan,
    Routine,
    Step,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuccessCriteriaEvaluationStatus {
    Missing,
    Defined,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuccessCriteriaEvaluationEntry {
    pub subject: SuccessCriteriaSubjectKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default)]
    pub required_artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_viable_completion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_window_hours: Option<u32>,
    #[serde(default)]
    pub declared_fields: Vec<String>,
    pub status: SuccessCriteriaEvaluationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SuccessCriteriaEvaluationReport {
    #[serde(default)]
    pub total_subjects: usize,
    #[serde(default)]
    pub defined_count: usize,
    #[serde(default)]
    pub missing_count: usize,
    #[serde(default)]
    pub entries: Vec<SuccessCriteriaEvaluationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StepFailurePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_missing_connector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_model_failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StepRetryPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StepCostRate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_usd_per_token: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_usd_per_token: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepCostProvenance {
    pub step_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_out: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_at_execution_time: Option<StepCostRate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computed_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cumulative_run_cost_usd_at_step_end: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_warning_fired: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_limit_reached: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StepProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_provenance: Option<StepCostProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepPackage {
    pub step_id: String,
    pub label: String,
    pub kind: String,
    pub action: String,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub context_reads: Vec<String>,
    #[serde(default)]
    pub context_writes: Vec<String>,
    #[serde(default)]
    pub connector_requirements: Vec<ConnectorRequirement>,
    #[serde(default)]
    pub model_policy: StepModelPolicy,
    pub approval_policy: ApprovalMode,
    #[serde(default)]
    pub success_criteria: SuccessCriteria,
    #[serde(default)]
    pub failure_policy: StepFailurePolicy,
    #[serde(default)]
    pub retry_policy: StepRetryPolicy,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<StepProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutinePackage {
    pub routine_id: String,
    pub semantic_kind: RoutineSemanticKind,
    pub trigger: TriggerDefinition,
    #[serde(default)]
    pub dependencies: Vec<RoutineDependency>,
    pub dependency_resolution: DependencyResolution,
    #[serde(default)]
    pub connector_resolution: RoutineConnectorResolution,
    pub data_scope: DataScope,
    pub audit_scope: AuditScope,
    #[serde(default)]
    pub success_criteria: SuccessCriteria,
    #[serde(default)]
    pub steps: Vec<StepPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectorIntent {
    pub capability: String,
    pub why: String,
    pub required: bool,
    pub degraded_mode_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ConnectorBindingResolutionEntry {
    pub capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub required: bool,
    pub degraded_mode_allowed: bool,
    pub resolved: bool,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ConnectorBindingResolutionReport {
    #[serde(default)]
    pub mapped_count: usize,
    #[serde(default)]
    pub unresolved_required_count: usize,
    #[serde(default)]
    pub unresolved_optional_count: usize,
    #[serde(default)]
    pub entries: Vec<ConnectorBindingResolutionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectorBinding {
    pub capability: String,
    pub binding_type: String,
    pub binding_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_pattern: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialBindingRef {
    pub capability: String,
    pub binding_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CredentialEnvelope {
    pub routine_id: String,
    #[serde(default)]
    pub entitled_connectors: Vec<CredentialBindingRef>,
    #[serde(default)]
    pub denied_connectors: Vec<CredentialBindingRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope_issued_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuing_authority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BudgetPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_per_run_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_daily_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_weekly_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_ceiling_per_run: Option<u64>,
    #[serde(default)]
    pub cheap_model_preferred_for: Vec<String>,
    #[serde(default)]
    pub strong_model_reserved_for: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CostTrackingUnit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default)]
    pub recorded_fields: Vec<String>,
    #[serde(default)]
    pub tracking_scope: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetWindowEnforcement {
    pub window: String,
    pub on_limit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DailyAndWeeklyEnforcement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily: Option<BudgetWindowEnforcement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly: Option<BudgetWindowEnforcement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BudgetEnforcement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_tracking_unit: Option<CostTrackingUnit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_warning_threshold: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_limit_behavior: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_result_preservation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_and_weekly_enforcement: Option<DailyAndWeeklyEnforcement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ApprovalMatrix {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_posts: Option<ApprovalMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_replies: Option<ApprovalMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outbound_email: Option<ApprovalMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_reports: Option<ApprovalMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector_mutations: Option<ApprovalMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_actions: Option<ApprovalMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterRoutinePolicy {
    pub communication_model: CommunicationModel,
    pub shared_memory_access: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_memory_justification: Option<String>,
    pub peer_visibility: PeerVisibility,
    pub artifact_handoff_validation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TriggerPolicy {
    #[serde(default)]
    pub supported: Vec<TriggerKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OutputRoots {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drafts: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PlanValidationState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_connectors_mapped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directories_writable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedules_valid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_resolved: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies_resolvable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals_complete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded_modes_acknowledged: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_scopes_valid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_scopes_valid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_context_scopes_valid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inter_routine_policy_complete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_envelopes_valid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compartmentalized_activation_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_objects_valid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria_evaluation: Option<SuccessCriteriaEvaluationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OverlapIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_hash: Option<String>,
    #[serde(default)]
    pub normalized_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SemanticIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity_threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverlapLogEntry {
    pub matched_plan_id: String,
    pub matched_plan_revision: u32,
    pub match_layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity_score: Option<f64>,
    pub decision: String,
    pub decided_by: String,
    pub decided_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OverlapPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_identity: Option<OverlapIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_identity: Option<SemanticIdentity>,
    #[serde(default)]
    pub overlap_log: Vec<OverlapLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanPackage {
    pub plan_id: String,
    pub plan_revision: u32,
    pub lifecycle_state: PlanLifecycleState,
    pub owner: PlanOwner,
    pub mission: MissionDefinition,
    #[serde(default)]
    pub success_criteria: SuccessCriteria,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_policy: Option<BudgetPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_enforcement: Option<BudgetEnforcement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<ApprovalMatrix>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inter_routine_policy: Option<InterRoutinePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_policy: Option<TriggerPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_roots: Option<OutputRoots>,
    #[serde(default)]
    pub precedence_log: Vec<PrecedenceLogEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_diff: Option<PlanDiff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_trigger_record: Option<ManualTriggerRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_state: Option<PlanValidationState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlap_policy: Option<OverlapPolicy>,
    #[serde(default)]
    pub routine_graph: Vec<RoutinePackage>,
    #[serde(default)]
    pub connector_intents: Vec<ConnectorIntent>,
    #[serde(default)]
    pub connector_bindings: Vec<ConnectorBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector_binding_resolution: Option<ConnectorBindingResolutionReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_routing_resolution: Option<ModelRoutingReport>,
    #[serde(default)]
    pub credential_envelopes: Vec<CredentialEnvelope>,
    #[serde(default)]
    pub context_objects: Vec<ContextObject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

pub fn with_manual_trigger_record(
    plan_package: &PlanPackage,
    trigger_id: &str,
    triggered_by: &str,
    trigger_source: ManualTriggerSource,
    dry_run: bool,
    triggered_at: &str,
    run_id: Option<&str>,
    outcome: Option<&str>,
    artifacts_produced: Vec<String>,
    notes: Option<&str>,
) -> Option<PlanPackage> {
    let routine_id = plan_package.routine_graph.first()?.routine_id.clone();
    let mut next = plan_package.clone();
    next.manual_trigger_record = Some(ManualTriggerRecord {
        trigger_id: trigger_id.to_string(),
        plan_id: next.plan_id.clone(),
        plan_revision: next.plan_revision,
        routine_id,
        triggered_by: triggered_by.to_string(),
        trigger_source,
        dry_run,
        approval_policy_snapshot: next.approval_policy.clone(),
        connector_binding_snapshot: next.connector_bindings.clone(),
        triggered_at: triggered_at.to_string(),
        run_id: run_id.map(str::to_string),
        outcome: outcome.map(str::to_string),
        artifacts_produced,
        notes: notes.map(str::to_string),
    });
    Some(next)
}

pub fn allowed_lifecycle_transitions(state: PlanLifecycleState) -> &'static [PlanLifecycleState] {
    match state {
        PlanLifecycleState::Draft => &[PlanLifecycleState::Preview, PlanLifecycleState::Archived],
        PlanLifecycleState::Preview => &[
            PlanLifecycleState::AwaitingApproval,
            PlanLifecycleState::Draft,
            PlanLifecycleState::Archived,
        ],
        PlanLifecycleState::AwaitingApproval => &[
            PlanLifecycleState::Approved,
            PlanLifecycleState::Preview,
            PlanLifecycleState::Draft,
            PlanLifecycleState::Archived,
        ],
        PlanLifecycleState::Approved => &[
            PlanLifecycleState::Applied,
            PlanLifecycleState::Preview,
            PlanLifecycleState::Draft,
            PlanLifecycleState::Superseded,
        ],
        PlanLifecycleState::Applied => &[
            PlanLifecycleState::Active,
            PlanLifecycleState::Paused,
            PlanLifecycleState::Superseded,
            PlanLifecycleState::Archived,
        ],
        PlanLifecycleState::Active => &[
            PlanLifecycleState::Degraded,
            PlanLifecycleState::Paused,
            PlanLifecycleState::Superseded,
            PlanLifecycleState::Archived,
        ],
        PlanLifecycleState::Degraded => &[
            PlanLifecycleState::Active,
            PlanLifecycleState::Paused,
            PlanLifecycleState::Superseded,
            PlanLifecycleState::Archived,
        ],
        PlanLifecycleState::Paused => &[
            PlanLifecycleState::Active,
            PlanLifecycleState::Degraded,
            PlanLifecycleState::Superseded,
            PlanLifecycleState::Archived,
        ],
        PlanLifecycleState::Superseded => {
            &[PlanLifecycleState::Archived, PlanLifecycleState::Draft]
        }
        PlanLifecycleState::Archived => &[PlanLifecycleState::Draft],
    }
}

pub fn can_transition_plan_lifecycle(from: PlanLifecycleState, to: PlanLifecycleState) -> bool {
    allowed_lifecycle_transitions(from).contains(&to)
}

fn default_dependency_resolution() -> DependencyResolution {
    DependencyResolution {
        strategy: DependencyResolutionStrategy::TopologicalSequential,
        partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
        reentry_point: ReentryPoint::FailedStep,
        mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
    }
}

fn default_connector_resolution() -> RoutineConnectorResolution {
    RoutineConnectorResolution {
        states: vec![
            "unresolved".to_string(),
            "options_ready".to_string(),
            "awaiting_user_choice".to_string(),
            "selected".to_string(),
            "bound".to_string(),
            "linked_to_revision".to_string(),
            "degraded_ready".to_string(),
            "activation_handed_off".to_string(),
            "blocked".to_string(),
            "deferred".to_string(),
        ],
        binding_options: vec![
            "mcp_server".to_string(),
            "native_feature".to_string(),
            "oauth_integration".to_string(),
            "manual_credential".to_string(),
            "http_adapter".to_string(),
        ],
    }
}

fn default_data_scope(workspace_root: &str, routine_id: &str) -> DataScope {
    let scoped_root =
        |kind: &str| format!("{workspace_root}/knowledge/workflows/{kind}/{routine_id}/**");
    DataScope {
        readable_paths: vec![
            "mission.goal".to_string(),
            scoped_root("plan"),
            scoped_root("drafts"),
            scoped_root("proof"),
            scoped_root("run-history"),
        ],
        writable_paths: vec![
            scoped_root("plan"),
            scoped_root("drafts"),
            scoped_root("proof"),
            scoped_root("run-history"),
        ],
        denied_paths: vec!["credentials/**".to_string()],
        cross_routine_visibility: CrossRoutineVisibility::None,
        mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
        mission_context_justification: None,
    }
}

fn default_audit_scope() -> AuditScope {
    AuditScope {
        run_history_visibility: RunHistoryVisibility::PlanOwner,
        named_audit_roles: Vec::new(),
        intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
        final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
    }
}

fn default_budget_policy() -> BudgetPolicy {
    BudgetPolicy {
        max_cost_per_run_usd: Some(4.0),
        max_daily_cost_usd: Some(20.0),
        max_weekly_cost_usd: Some(60.0),
        token_ceiling_per_run: Some(40_000),
        cheap_model_preferred_for: vec![
            "search".to_string(),
            "dedupe".to_string(),
            "clustering".to_string(),
            "bulk extraction".to_string(),
        ],
        strong_model_reserved_for: vec![
            "public copy".to_string(),
            "approval review".to_string(),
            "final synthesis".to_string(),
        ],
    }
}

fn default_budget_enforcement() -> BudgetEnforcement {
    BudgetEnforcement {
        cost_tracking_unit: Some(CostTrackingUnit {
            method: Some("token_count × model_rate_per_token".to_string()),
            recorded_fields: vec![
                "tokens_in".to_string(),
                "tokens_out".to_string(),
                "model_id".to_string(),
                "rate_at_execution_time".to_string(),
                "computed_cost_usd".to_string(),
            ],
            tracking_scope: vec![
                "step".to_string(),
                "routine".to_string(),
                "plan_run".to_string(),
            ],
        }),
        soft_warning_threshold: Some(0.8),
        hard_limit_behavior: Some("pause_before_step".to_string()),
        partial_result_preservation: Some(true),
        daily_and_weekly_enforcement: Some(DailyAndWeeklyEnforcement {
            daily: Some(BudgetWindowEnforcement {
                window: "rolling_24h".to_string(),
                on_limit: "defer_until_next_window".to_string(),
            }),
            weekly: Some(BudgetWindowEnforcement {
                window: "rolling_7d".to_string(),
                on_limit: "block_and_request_review".to_string(),
            }),
        }),
    }
}

fn re_root_path(workspace_root: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        workspace_root.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

fn default_output_roots(workspace_root: &str) -> OutputRoots {
    OutputRoots {
        plan: Some(re_root_path(workspace_root, "knowledge/workflows/plan/")),
        history: Some(re_root_path(
            workspace_root,
            "knowledge/workflows/run-history/",
        )),
        proof: Some(re_root_path(workspace_root, "knowledge/workflows/proof/")),
        drafts: Some(re_root_path(workspace_root, "knowledge/workflows/drafts/")),
    }
}

fn required_capabilities_for_routine(
    routine: &RoutinePackage,
) -> std::collections::BTreeSet<String> {
    let mut required_capabilities = std::collections::BTreeSet::new();
    for step in &routine.steps {
        for requirement in &step.connector_requirements {
            required_capabilities.insert(requirement.capability.clone());
        }
    }
    required_capabilities
}

pub fn derive_connector_binding_resolution_for_plan(
    plan: &PlanPackage,
) -> ConnectorBindingResolutionReport {
    let mut entries_by_capability =
        std::collections::BTreeMap::<String, ConnectorBindingResolutionEntry>::new();

    for intent in &plan.connector_intents {
        entries_by_capability.insert(
            intent.capability.clone(),
            ConnectorBindingResolutionEntry {
                capability: intent.capability.clone(),
                why: Some(intent.why.clone()),
                required: intent.required,
                degraded_mode_allowed: intent.degraded_mode_allowed,
                resolved: false,
                status: if intent.required {
                    "unresolved_required".to_string()
                } else {
                    "unresolved_optional".to_string()
                },
                binding_type: None,
                binding_id: None,
                allowlist_pattern: None,
            },
        );
    }

    for binding in &plan.connector_bindings {
        let entry = entries_by_capability
            .entry(binding.capability.clone())
            .or_insert_with(|| ConnectorBindingResolutionEntry {
                capability: binding.capability.clone(),
                why: None,
                required: false,
                degraded_mode_allowed: false,
                resolved: false,
                status: "unresolved_optional".to_string(),
                binding_type: None,
                binding_id: None,
                allowlist_pattern: None,
            });
        entry.binding_type = Some(binding.binding_type.clone());
        entry.binding_id = Some(binding.binding_id.clone());
        entry.allowlist_pattern = binding.allowlist_pattern.clone();
        if binding.status == "mapped" {
            entry.resolved = true;
            entry.status = "mapped".to_string();
        } else if entry.required {
            entry.status = "unresolved_required".to_string();
        } else {
            entry.status = "unresolved_optional".to_string();
        }
    }

    let mut entries = entries_by_capability.into_values().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.capability.cmp(&right.capability));

    let mapped_count = entries.iter().filter(|entry| entry.resolved).count();
    let unresolved_required_count = entries
        .iter()
        .filter(|entry| !entry.resolved && entry.required)
        .count();
    let unresolved_optional_count = entries
        .iter()
        .filter(|entry| !entry.resolved && !entry.required)
        .count();

    ConnectorBindingResolutionReport {
        mapped_count,
        unresolved_required_count,
        unresolved_optional_count,
        entries,
    }
}

