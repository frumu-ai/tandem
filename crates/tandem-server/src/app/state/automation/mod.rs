use std::collections::HashSet;
use std::path::{Component, PathBuf};
use std::time::Duration;

pub(crate) mod assessment;
pub(crate) mod capability_impl;
pub(crate) mod enforcement;
pub(crate) mod extraction;
pub(crate) use extraction::{
    detect_glob_loop, extract_recoverable_json_artifact,
    extract_recoverable_json_artifact_prefer_standup, extract_session_text_output,
};
pub(crate) mod legacy_defaults;
pub(crate) mod lifecycle;
pub(crate) mod node_output;
pub(crate) mod node_runtime_impl;
pub(crate) mod path_hygiene;
pub(crate) mod prompting_impl;
pub(crate) mod rate_limit;
pub(crate) mod receipts;
pub(crate) mod scheduler;
pub(crate) mod types;
pub(crate) mod upstream;
pub(crate) mod validation;
pub(crate) mod verification;
mod workflow_impl;
pub(crate) mod workflow_learning;
use assessment::*;
pub(crate) use capability_impl::*;
use enforcement::*;
use extraction::*;
pub(crate) use legacy_defaults::{
    automation_node_allows_attachments, automation_node_builder_metadata,
    automation_node_builder_string_array, automation_node_delivery_method,
    automation_node_delivery_target, automation_node_email_content_type,
    automation_node_inline_body_only, automation_node_is_outbound_action,
    automation_node_is_research_finalize, automation_node_preserves_full_upstream_inputs,
    automation_node_requires_email_delivery, automation_node_uses_upstream_validation_evidence,
};
use lifecycle::*;
pub use lifecycle::{record_automation_lifecycle_event, record_automation_workflow_state_events};
pub(crate) use node_output::enrich_automation_node_output_for_contract;
pub(crate) use node_output::research_required_next_tool_actions;
use node_output::*;
use path_hygiene::*;
use receipts::*;
pub use scheduler::{
    AutomationScheduler, PreexistingArtifactRegistry, QueueReason, SchedulerMetadata,
    ValidatedArtifact,
};
use types::*;
use upstream::*;
use validation::*;
use verification::*;
pub(crate) use workflow_impl::{
    automation_builder_declared_output_targets, canonicalize_automation_output_paths,
    infer_automation_output_contract, migrate_bundled_studio_research_split_automation,
    repair_automation_output_contracts,
};
pub(crate) use workflow_learning::*;

pub fn automation_node_output_enforcement(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputEnforcement {
    enforcement::automation_node_output_enforcement(node)
}

pub(crate) fn automation_node_research_stage(node: &AutomationFlowNode) -> Option<String> {
    legacy_defaults::automation_node_research_stage(node)
}

pub(crate) async fn resolve_automation_agent_template(
    state: &AppState,
    workspace_root: &str,
    template_id: &str,
) -> anyhow::Result<Option<tandem_orchestrator::AgentTemplate>> {
    let template_id = template_id.trim();
    if template_id.is_empty() {
        return Ok(None);
    }

    if let Some(template) = state
        .agent_teams
        .get_template_for_workspace(workspace_root, template_id)
        .await?
    {
        return Ok(Some(template));
    }

    let global_workspace_root = state.workspace_index.snapshot().await.root;
    if global_workspace_root == workspace_root {
        return Ok(None);
    }

    state
        .agent_teams
        .get_template_for_workspace(&global_workspace_root, template_id)
        .await
}

use serde_json::{json, Value};
use tandem_core::resolve_shared_paths;
use tandem_memory::MemoryManager;
use tandem_plan_compiler::api as compiler_api;
use tandem_types::{
    MessagePart, MessagePartInput, MessageRole, ModelSpec, PrewriteCoverageMode,
    PrewriteRequirements, SendMessageRequest, Session, ToolMode,
};

use super::*;
use crate::capability_resolver::{self};
use crate::config::{self};
use crate::util::time::now_ms;

mod logic;
pub(crate) use logic::*;

#[cfg(test)]
mod tests;
