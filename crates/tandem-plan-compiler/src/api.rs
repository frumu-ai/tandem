// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1
//
//! Curated public API surface for embedding the mission compiler.
//!
//! Hosts should prefer importing from this module instead of depending on the
//! crate's internal module layout directly.
//!
//! New external consumers should treat `api` as the only supported surface.

pub use crate::automation_projection::{
    ProjectedAutomationAgentProfile, ProjectedAutomationApprovalGate, ProjectedAutomationDraft,
    ProjectedAutomationExecutionPolicy, ProjectedAutomationNode, ProjectedAutomationStageKind,
};
pub use crate::contracts::{
    approval_gate_output_contract_seed, build_workflow_plan_with_planner_json,
    code_patch_output_contract_seed, compare_workflow_plan_preview_replay_with_revision,
    compile_workflow_plan_preview_package_with_revision, default_execute_goal_output_contract_seed,
    default_fallback_schedule_json, default_fallback_step_json, output_contract_seed,
    projected_output_validator_kind_from_key, research_output_contract_policy_seed,
    review_summary_output_contract_seed, revise_workflow_plan_draft_json, workflow_plan_to_json,
    OutputContractPolicySeed, OutputContractSeed, PlannerBuildRequestJson, PlannerBuildResultJson,
    PlannerDraftRevisionResultJson, ProjectedOutputValidatorKind, WorkflowPlanDraftRecordJson,
    WorkflowPlanJson, WorkflowPlanStepJson,
};
pub use crate::dependency_planner::{
    plan_routine_execution, DependencyPlanningError, RoutineExecutionBatch, RoutineExecutionPlan,
};
pub use crate::host::{
    Clock, McpToolCatalog, PlanStore, PlannerLlmInvocation, PlannerLlmInvoker,
    PlannerModelRegistry, PlannerSessionStore, TelemetrySink, WorkspaceResolver,
};
pub use crate::materialization::{
    approved_plan_materialization, approved_plan_success_memory_value,
    materialization_seed_from_projection, project_plan_context_materialization,
    ApprovedPlanMaterialization, ApprovedPlanRoutineMaterialization,
    ApprovedPlanStepContextBinding, ProjectedAutomationContextMaterialization,
    ProjectedAutomationMaterializationSeed, ProjectedRoutineContextPartition,
    ProjectedStepContextBindings,
};
pub use crate::mission_preview::{
    compile_mission_blueprint_preview, summarize_mission_coder_run_handoffs,
    summarize_mission_execution_boundary, CompiledNodePreview, MissionBlueprintPreview,
    MissionCoderRunHandoffCandidate, MissionExecutionBoundarySummary,
};
pub use crate::mission_runtime::{
    compile_mission_runtime_projection, project_mission_runtime_materialization_seed,
    ProjectedMissionInputRef,
};
pub use crate::plan_bundle::{
    compare_plan_package_replay, export_plan_package_bundle, preview_plan_package_import_bundle,
    validate_plan_package_bundle, PlanPackageExportBundle, PlanPackageImportBundle,
    PlanPackageImportPreview, PlanReplayDiffEntry, PlanReplayIssue, PlanReplayReport,
    PlanScopeSnapshot, RoutineScopeSnapshot,
};
pub use crate::plan_overlap::{
    analyze_plan_overlap, overlap_log_entry_from_analysis, OverlapComparison, OverlapDecision,
    OverlapMatchLayer,
};
pub use crate::plan_package::{
    allowed_lifecycle_transitions, can_transition_plan_lifecycle,
    compile_workflow_plan_preview_package, derive_connector_binding_resolution_for_plan,
    derive_credential_envelopes_for_plan, derive_model_routing_resolution_for_plan,
    with_manual_trigger_record, ApprovalMatrix, ApprovalMode, AuditScope, BudgetEnforcement,
    BudgetPolicy, BudgetWindowEnforcement, CommunicationModel, ConnectorBinding,
    ConnectorBindingResolutionEntry, ConnectorBindingResolutionReport, ConnectorIntent,
    ConnectorRequirement, ContextObject, ContextObjectProvenance, ContextObjectScope,
    ContextValidationStatus, CostTrackingUnit, CredentialBindingRef, CredentialEnvelope,
    CrossRoutineVisibility, DailyAndWeeklyEnforcement, DataScope, DependencyMode,
    DependencyResolution, DependencyResolutionStrategy, FinalArtifactVisibility,
    InterRoutinePolicy, IntermediateArtifactVisibility, ManualTriggerRecord, ManualTriggerSource,
    MidRoutineConnectorFailureMode, MissionContextScope, MissionDefinition, ModelRoutingEntry,
    ModelRoutingReport, ModelTier, OutputRoots, OverlapIdentity, OverlapLogEntry, OverlapPolicy,
    PartialFailureMode, PeerVisibility, PlanDiff, PlanDiffChangeType, PlanDiffChangedField,
    PlanDiffSummary, PlanLifecycleState, PlanOwner, PlanPackage, PlanValidationState,
    PrecedenceLogEntry, PrecedenceSourceTier, ReentryPoint, RoutineConnectorResolution,
    RoutineDependency, RoutinePackage, RoutineSemanticKind, RunHistoryVisibility,
    StepCostProvenance, StepCostRate, StepFailurePolicy, StepModelPolicy, StepModelSelection,
    StepPackage, StepProvenance, StepRetryPolicy, SuccessCriteria, SuccessCriteriaEvaluationEntry,
    SuccessCriteriaEvaluationReport, SuccessCriteriaEvaluationStatus, SuccessCriteriaSubjectKind,
    TriggerDefinition, TriggerKind, TriggerPolicy,
};
pub use crate::plan_validation::{
    validate_plan_package, PlanValidationIssue, PlanValidationReport, PlanValidationSeverity,
};
pub use crate::planner_build::{
    build_workflow_plan_with_planner, prepare_build_request, PlannerBuildConfig,
    PlannerBuildRequest, PlannerBuildResult,
};
pub use crate::planner_drafts::{
    draft_not_found_response, load_workflow_plan_draft, reset_workflow_plan_draft,
    revise_workflow_plan_draft, store_chat_start_draft, store_preview_draft, PlannerDraftError,
    PlannerDraftRevisionResult,
};
pub use crate::planner_loop::{revise_workflow_plan_with_planner_loop, PlannerLoopConfig};
pub use crate::planner_messages::{planner_teaching_library_summary, TeachingLibrarySummary};
pub use crate::planner_session::{begin_planner_session, finish_planner_session};
pub use crate::planner_types::{PlannerClarifier, PlannerInvocationFailure};
pub use crate::runtime_projection::{
    compile_workflow_runtime_projection, project_workflow_runtime_materialization_seed,
};
pub use crate::workflow_plan::{
    build_planner_capability_summary, extract_json_value_from_text, manual_schedule,
    normalize_workflow_step_metadata, output_contract_is_code_patch,
    output_contract_is_research_brief, pack_builder_export_args, plan_step_with_dep,
    planner_model_spec, resolve_workspace_root_candidate, schedule_from_value,
    validate_workflow_plan, PackBuilderExportOptions, PlannerMcpServerToolSet,
    WorkflowInputRefLike,
};
