#![recursion_limit = "512"]

use std::ops::Deref;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use futures::future::{join_all, BoxFuture};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_memory::types::MemoryTier;
use tandem_orchestrator::MissionState;
use tandem_types::{
    EngineEvent, HostRuntimeContext, MessagePart, MessagePartInput, MessageRole, ModelSpec,
    PrewriteCoverageMode, PrewriteRequirements, SendMessageRequest, Session, ToolMode,
};
use tokio::fs;
use tokio::sync::RwLock;

use tandem_channels::config::{ChannelsConfig, DiscordConfig, SlackConfig, TelegramConfig};
use tandem_core::{resolve_shared_paths, PromptContextHook, PromptContextHookContext};
use tandem_memory::db::MemoryDatabase;
use tandem_providers::ChatMessage;
use tandem_workflows::{
    load_registry as load_workflow_registry, validate_registry as validate_workflow_registry,
    WorkflowHookBinding, WorkflowLoadSource, WorkflowRegistry, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowSourceKind, WorkflowSourceRef, WorkflowSpec,
    WorkflowValidationMessage,
};

mod agent_teams;
pub mod app;
mod automation_v2;
mod browser;
mod bug_monitor;
mod bug_monitor_github;
mod capability_resolver;
mod config;
mod http;
mod mcp_catalog;
mod memory;
mod pack_builder;
mod pack_manager;
mod preset_composer;
mod preset_registry;
mod preset_summary;
mod routines;
mod runtime;
mod shared_resources;
mod util;
pub mod webui;
mod workflows;

pub use agent_teams::AgentTeamRuntime;
pub use automation_v2::types::{
    AutomationAgentMcpPolicy, AutomationAgentProfile, AutomationAgentToolPolicy,
    AutomationApprovalGate, AutomationExecutionPolicy, AutomationFailureRecord,
    AutomationFlowInputRef, AutomationFlowNode, AutomationFlowOutputContract, AutomationFlowSpec,
    AutomationGateDecisionRecord, AutomationLifecycleRecord, AutomationNodeOutput,
    AutomationNodeStageKind, AutomationOutputValidatorKind, AutomationPendingGate,
    AutomationRunCheckpoint, AutomationRunStatus, AutomationStopKind, AutomationV2RunRecord,
    AutomationV2Schedule, AutomationV2ScheduleType, AutomationV2Spec, AutomationV2Status,
    AutomationValidatorSummary, WorkflowPlan, WorkflowPlanChatMessage, WorkflowPlanConversation,
    WorkflowPlanDraftRecord, WorkflowPlanStep,
};
pub use browser::{
    install_browser_sidecar, BrowserHealthSummary, BrowserSidecarInstallResult,
    BrowserSmokeTestResult, BrowserSubsystem,
};
pub use bug_monitor::types::{
    BugMonitorBindingCandidate, BugMonitorCapabilityMatch, BugMonitorCapabilityReadiness,
    BugMonitorConfig, BugMonitorDraftRecord, BugMonitorIncidentRecord, BugMonitorLabelMode,
    BugMonitorPostRecord, BugMonitorProviderPreference, BugMonitorReadiness,
    BugMonitorRuntimeStatus, BugMonitorStatus, BugMonitorSubmission,
};
pub use capability_resolver::CapabilityResolver;
pub use http::serve;
pub use memory::types::{GovernedMemoryRecord, MemoryAuditEvent};
pub use pack_manager::PackManager;
pub use preset_composer::PromptComposeInput;
pub use preset_registry::PresetRegistry;
pub use routines::errors::RoutineStoreError;
pub use routines::types::{
    ExternalActionRecord, RoutineHistoryEvent, RoutineMisfirePolicy, RoutineRunArtifact,
    RoutineRunRecord, RoutineRunStatus, RoutineSchedule, RoutineSessionPolicy, RoutineSpec,
    RoutineStatus, RoutineTriggerPlan,
};
pub use runtime::lease::EngineLease;
pub use runtime::runs::{ActiveRun, RunRegistry};
pub use runtime::state::RuntimeState;
pub use runtime::worktrees::ManagedWorktreeRecord;
pub use shared_resources::types::{ResourceConflict, ResourceStoreError, SharedResourceRecord};
pub use util::build::{binary_path_for_health, build_id};
pub use util::host::detect_host_runtime_context;
pub use util::time::now_ms;
pub use workflows::{
    canonical_workflow_event_names, dispatch_workflow_event, execute_hook_binding,
    execute_workflow, parse_workflow_action, run_workflow_dispatcher, simulate_workflow_event,
};

pub(crate) fn normalize_absolute_workspace_root(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("workspace_root is required".to_string());
    }
    let as_path = PathBuf::from(trimmed);
    if !as_path.is_absolute() {
        return Err("workspace_root must be an absolute path".to_string());
    }
    tandem_core::normalize_workspace_path(trimmed)
        .ok_or_else(|| "workspace_root is invalid".to_string())
}

// Re-exports from app::state (impl AppState moved here)
pub use crate::app::state::{
    derive_status_index_update, evaluate_routine_execution_policy, sha256_hex, truncate_text,
    AppState, ChannelRuntime, ChannelStatus, RoutineExecutionDecision, StatusIndexUpdate,
};

pub use crate::app::startup::{StartupSnapshot, StartupState, StartupStatus};

pub use config::channels::{
    ChannelsConfigFile, DiscordConfigFile, SlackConfigFile, TelegramConfigFile,
};
pub use config::webui::normalize_web_ui_prefix;
pub use config::webui::WebUiConfig;

// Also need normalize_allowed_users_or_wildcard?
pub use crate::app::state::extract_persistable_tool_part;
pub use crate::app::state::run_automation_v2_executor;
pub use crate::app::state::{
    automation_lifecycle_event_metadata_for_node, default_model_spec_from_effective_config,
    record_automation_lifecycle_event, record_automation_lifecycle_event_with_metadata,
    record_automation_workflow_state_events,
};
pub use crate::app::state::{
    automation_node_required_output_path, clear_automation_subtree_outputs,
    collect_automation_descendants, refresh_automation_runtime_state,
};
pub use crate::app::tasks::{
    run_agent_team_supervisor, run_automation_v2_scheduler, run_bug_monitor, run_routine_executor,
    run_routine_scheduler, run_session_context_run_journaler, run_session_part_persister,
    run_status_indexer, run_usage_aggregator,
};
pub use config::channels::normalize_allowed_tools;
pub use config::channels::normalize_allowed_users_or_wildcard;
