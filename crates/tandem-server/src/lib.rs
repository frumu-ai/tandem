#![recursion_limit = "256"]
#![allow(warnings)]

pub mod agent_teams;
pub mod app;
pub mod audit;
pub mod automation_v2;
pub mod browser;
pub mod bug_monitor;
pub mod bug_monitor_github;
pub mod capability_resolver;
pub mod config;
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
pub mod util;
pub mod webui;
pub mod workflows;

pub use app::startup::*;
pub use app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata;
pub use app::state::*;
pub use app::tasks::run_session_context_run_journaler;
pub use automation_v2::types::*;
pub use browser::*;
pub use bug_monitor::types::*;
pub use config::channels::*;
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
    dispatch_workflow_event, execute_workflow, run_workflow_dispatcher, simulate_workflow_event,
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
