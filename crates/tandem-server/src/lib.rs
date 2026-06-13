#![recursion_limit = "256"]
#![allow(warnings)]

pub mod agent_teams;
pub mod app;
pub mod audit;
pub mod automation_v2;
pub mod benchmarking;
#[cfg(feature = "browser")]
pub mod browser;
pub mod bug_monitor;
pub mod bug_monitor_github;
pub mod capability_resolver;
pub mod config;
pub mod eval;
pub mod failures;
pub mod goal_capability_learning;
pub mod http;
pub mod mcp_catalog;
pub mod mcp_catalog_generated;
pub mod memory;
pub mod optimization;
pub mod pack_builder;
pub mod pack_manager;
pub mod preset_composer;
pub mod preset_registry;
pub mod preset_summary;
pub mod routines;
pub mod runtime;
pub mod shared_resources;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod util;
pub mod webui;
pub mod workflow_learning_policy;
pub mod workflows;

pub use app::startup::*;
pub use app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata;
pub use app::state::*;
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
pub use bug_monitor::types::*;
pub use config::channels::*;
pub use eval::dataset::{
    ArtifactStatus, EvalDataset, EvalExpectedOutput, EvalTestCase, MetricTolerance,
};
pub use failures::{
    categorize_failure, classify_error_text, should_retry, AIFailureMode, FailureCategoryKind,
    FailureContext,
};
pub use http::*;
pub use memory::types::*;
pub use optimization::*;
pub use routines::errors::*;
pub use routines::types::*;
pub use runtime::lease::*;
pub use runtime::runs::*;
pub use runtime::state::*;
pub use runtime::worktrees::*;
pub use shared_resources::types::*;
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
