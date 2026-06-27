pub mod context_metadata;
pub mod execution_profile;
pub mod governance;
pub mod mcp_policy;
pub mod routine;
pub mod run_mutability;
pub mod scheduler;
pub mod types;
pub mod webhooks;

#[cfg(test)]
mod execution_profile_tests;
#[cfg(test)]
mod types_tests;

pub use context_metadata::shared_context_pack_ids_from_metadata;
pub use execution_profile::{
    aggregate_human_dispositions_by_class, augment_output_with_profile_relaxation,
    classify_unmet_requirement, decide_profile_validation, effective_repair_budget,
    parse_execution_profile_str, parse_human_disposition_str, parse_validator_class_list,
    propagate_experimental_input_taint, set_human_disposition_on_output,
    tenant_default_execution_profile_from_env, tenant_relaxation_denylist_from_env,
    DispositionCounts, ExecutionProfile, HumanDisposition, ProfileValidationDecision,
    RelaxedValidatorClass, ValidationOutcome, ValidatorClass, ValidatorClassDispositionSummary,
};
pub use governance::*;
pub use mcp_policy::{AutomationAgentMcpPolicy, AutomationMcpConnectionGrant, AutomationMcpRunAs};
pub use routine::RoutineMisfirePolicy;
pub use scheduler::{QueueReason, SchedulerMetadata};
pub use types::*;
pub use webhooks::*;

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
