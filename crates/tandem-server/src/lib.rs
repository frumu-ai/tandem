#![recursion_limit = "256"]
// TAN-200 narrows the old crate-wide `allow(warnings)` blanket. The remaining
// warning backlog is explicit so it can be paid down while production panic
// lints are denied in the first guarded modules.
#![allow(
    unknown_lints,
    private_interfaces,
    unused,
    clippy::clone_on_copy,
    clippy::cloned_ref_to_slice_refs,
    clippy::cmp_owned,
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::collapsible_str_replace,
    clippy::derivable_impls,
    clippy::empty_line_after_doc_comments,
    clippy::expect_used,
    clippy::explicit_counter_loop,
    clippy::if_same_then_else,
    clippy::iter_overeager_cloned,
    clippy::large_enum_variant,
    clippy::manual_clamp,
    clippy::manual_flatten,
    clippy::manual_inspect,
    clippy::manual_is_multiple_of,
    clippy::manual_option_zip,
    clippy::manual_pattern_char_comparison,
    clippy::manual_split_once,
    clippy::map_entry,
    clippy::map_identity,
    clippy::match_like_matches_macro,
    clippy::needless_as_bytes,
    clippy::needless_borrow,
    clippy::needless_lifetimes,
    clippy::needless_question_mark,
    clippy::needless_range_loop,
    clippy::nonminimal_bool,
    clippy::option_map_unit_fn,
    clippy::ptr_arg,
    clippy::question_mark,
    clippy::redundant_closure,
    clippy::redundant_locals,
    clippy::result_large_err,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_cast,
    clippy::unnecessary_get_then_check,
    clippy::unnecessary_lazy_evaluations,
    clippy::unnecessary_literal_unwrap,
    clippy::unnecessary_map_or,
    clippy::unnecessary_sort_by,
    clippy::unwrap_or_default,
    clippy::unwrap_used,
    clippy::useless_asref,
    clippy::useless_conversion,
    clippy::vec_init_then_push
)]

pub mod acme_demo;
pub mod agent_teams;
pub mod app;
pub mod audit;
pub mod automation_v2;
pub mod benchmarking;
#[cfg(feature = "browser")]
pub mod browser;
pub mod capability_resolver;
pub mod config;
pub mod data_boundary_bridge;
pub(crate) mod encrypted_file_store;
pub mod eval_support;
pub mod failures;
pub mod goal_capability_learning;
pub(crate) mod governance_store;
pub mod http;
pub mod incident_monitor;
pub mod incident_monitor_github;
pub mod incident_monitor_governance_metrics;
pub mod incident_monitor_linear;
pub mod incident_monitor_local;
pub mod incident_monitor_mcp;
pub mod incident_monitor_reassessment;
pub mod incident_monitor_scenarios;
pub mod incident_monitor_webhook;
pub mod mcp_catalog;
pub mod mcp_catalog_generated;
pub mod memory;
pub mod optimization;
pub mod pack_builder;
pub mod pack_manager;
pub mod preset_composer;
pub mod preset_registry;
pub mod preset_summary;
pub(crate) mod provider_egress;
pub mod routines;
pub mod runtime;
pub mod runtime_event_log;
pub mod shared_resources;
pub mod signal_triage;
pub mod stateful_runtime;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod util;
pub mod webui;
pub mod workflow_learning_policy;
pub mod workflows;

pub use app::startup::*;
pub use app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata;
pub use app::state::*;
pub use app::tasks::run_automation_webhook_retention_reaper;
pub use app::tasks::run_runtime_event_log_persister;
pub use app::tasks::run_session_context_run_journaler;
pub use automation_v2::execution_profile::{
    aggregate_human_dispositions_by_class, augment_output_with_profile_relaxation,
    classify_unmet_requirement, decide_profile_validation, effective_repair_budget,
    parse_execution_profile_str, parse_human_disposition_str, parse_validator_class_list,
    propagate_experimental_input_taint, set_human_disposition_on_output,
    tenant_default_execution_profile_from_env, tenant_relaxation_denylist_from_env,
    DispositionCounts, ExecutionProfile, HumanDisposition, ProfileValidationDecision,
    RelaxedValidatorClass, ValidationOutcome, ValidatorClass, ValidatorClassDispositionSummary,
};
pub use automation_v2::types::*;
#[cfg(feature = "browser")]
pub use browser::*;
pub use config::channels::*;
pub use config::engine::{config_reference_markdown, EngineConfigOptions, EngineConfigReport};
pub use failures::{
    categorize_failure, classify_error_text, should_retry, AIFailureMode, FailureCategoryKind,
    FailureContext,
};
pub use http::*;
pub use incident_monitor::governance_metrics::IncidentMonitorGovernanceThresholds;
pub use incident_monitor::reassessment::{
    IncidentMonitorReassessmentConfig, ReassessmentComparison, ReassessmentFinding,
    ReassessmentRecord, ReassessmentScheduleStatus, ReassessmentTrigger,
};
pub use incident_monitor::scenarios::{
    default_scenario_pack, IncidentMonitorScenario, IncidentMonitorScenarioExpectation,
    IncidentMonitorScenarioInput, IncidentMonitorScenarioPack,
};
pub use incident_monitor::types::*;
pub use incident_monitor_reassessment::{
    run_incident_monitor_reassessment_scheduler, IncidentMonitorReassessmentPending,
};
pub use memory::types::*;
pub use optimization::*;
pub use routines::errors::*;
pub use routines::types::*;
pub use runtime::lease::*;
pub use runtime::runs::*;
pub use runtime::state::*;
pub use runtime::worktrees::*;
pub use shared_resources::types::*;
pub use signal_triage::*;
pub use stateful_runtime::*;
pub use tandem_types::EngineEvent;
pub use tandem_workflows::{WorkflowRunRecord, WorkflowRunStatus, WorkflowSourceRef};
pub use util::build::*;
pub use util::host::*;
pub use util::time::*;
pub use workflows::{
    dispatch_workflow_event, execute_workflow, resume_workflow_run, run_workflow_dispatcher,
    simulate_workflow_event,
};

pub fn normalize_absolute_workspace_root(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("workspace_root is required".to_string());
    }
    let as_path = std::path::PathBuf::from(trimmed);
    if !as_path.is_absolute() {
        return Err("workspace_root must be an absolute path".to_string());
    }
    tandem_core::normalize_workspace_path(trimmed)
        .ok_or_else(|| "workspace_root is invalid".to_string())
}
