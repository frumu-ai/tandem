#![recursion_limit = "512"]

use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use futures::future::{join_all, BoxFuture};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_memory::types::MemoryTier;
use tandem_memory::{GovernedMemoryTier, MemoryClassification, MemoryContentKind, MemoryPartition};
use tandem_orchestrator::MissionState;
use tandem_types::{
    EngineEvent, HostOs, HostRuntimeContext, MessagePart, MessagePartInput, MessageRole, ModelSpec,
    PathStyle, SendMessageRequest, Session, ShellFamily,
};
use tokio::fs;
use tokio::sync::RwLock;

use tandem_channels::config::{ChannelsConfig, DiscordConfig, SlackConfig, TelegramConfig};
use tandem_core::{
    resolve_shared_paths, AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus,
    PermissionManager, PluginRegistry, PromptContextHook, PromptContextHookContext, Storage,
};
use tandem_memory::db::MemoryDatabase;
use tandem_providers::ChatMessage;
use tandem_providers::ProviderRegistry;
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::ToolRegistry;
use tandem_workflows::{
    load_registry as load_workflow_registry, validate_registry as validate_workflow_registry,
    WorkflowHookBinding, WorkflowLoadSource, WorkflowRegistry, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowSourceKind, WorkflowSourceRef, WorkflowSpec,
    WorkflowValidationMessage,
};

mod agent_teams;
mod browser;
mod bug_monitor_github;
mod capability_resolver;
mod http;
mod mcp_catalog;
mod pack_builder;
mod pack_manager;
mod preset_composer;
mod preset_registry;
mod preset_summary;
pub mod webui;
mod workflows;

pub use agent_teams::AgentTeamRuntime;
pub use browser::{
    install_browser_sidecar, BrowserHealthSummary, BrowserSidecarInstallResult,
    BrowserSmokeTestResult, BrowserSubsystem,
};
pub use capability_resolver::CapabilityResolver;
pub use http::serve;
pub use pack_manager::PackManager;
pub use preset_composer::PromptComposeInput;
pub use preset_registry::PresetRegistry;
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelStatus {
    pub enabled: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub active_sessions: u64,
    pub meta: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebUiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_web_ui_prefix")]
    pub path_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfigFile {
    pub telegram: Option<TelegramConfigFile>,
    pub discord: Option<DiscordConfigFile>,
    pub slack: Option<SlackConfigFile>,
    #[serde(default)]
    pub tool_policy: tandem_channels::config::ChannelToolPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfigFile {
    pub bot_token: String,
    #[serde(default = "default_allow_all")]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub mention_only: bool,
    #[serde(default)]
    pub style_profile: tandem_channels::config::TelegramStyleProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfigFile {
    pub bot_token: String,
    #[serde(default)]
    pub guild_id: Option<String>,
    #[serde(default = "default_allow_all")]
    pub allowed_users: Vec<String>,
    #[serde(default = "default_discord_mention_only")]
    pub mention_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfigFile {
    pub bot_token: String,
    pub channel_id: String,
    #[serde(default = "default_allow_all")]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub mention_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EffectiveAppConfig {
    #[serde(default)]
    pub channels: ChannelsConfigFile,
    #[serde(default)]
    pub web_ui: WebUiConfig,
    #[serde(default)]
    pub browser: tandem_core::BrowserConfig,
    #[serde(default)]
    pub memory_consolidation: tandem_providers::MemoryConsolidationConfig,
}

#[derive(Default)]
pub struct ChannelRuntime {
    pub listeners: Option<tokio::task::JoinSet<()>>,
    pub statuses: std::collections::HashMap<String, ChannelStatus>,
}

#[derive(Debug, Clone)]
pub struct EngineLease {
    pub lease_id: String,
    pub client_id: String,
    pub client_type: String,
    pub acquired_at_ms: u64,
    pub last_renewed_at_ms: u64,
    pub ttl_ms: u64,
}

impl EngineLease {
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_renewed_at_ms) > self.ttl_ms
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveRun {
    #[serde(rename = "runID")]
    pub run_id: String,
    #[serde(rename = "startedAtMs")]
    pub started_at_ms: u64,
    #[serde(rename = "lastActivityAtMs")]
    pub last_activity_at_ms: u64,
    #[serde(rename = "clientID", skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(rename = "agentID", skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(rename = "agentProfile", skip_serializing_if = "Option::is_none")]
    pub agent_profile: Option<String>,
}

#[derive(Clone, Default)]
pub struct RunRegistry {
    active: Arc<RwLock<std::collections::HashMap<String, ActiveRun>>>,
}

impl RunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, session_id: &str) -> Option<ActiveRun> {
        self.active.read().await.get(session_id).cloned()
    }

    pub async fn acquire(
        &self,
        session_id: &str,
        run_id: String,
        client_id: Option<String>,
        agent_id: Option<String>,
        agent_profile: Option<String>,
    ) -> std::result::Result<ActiveRun, ActiveRun> {
        let mut guard = self.active.write().await;
        if let Some(existing) = guard.get(session_id).cloned() {
            return Err(existing);
        }
        let now = now_ms();
        let run = ActiveRun {
            run_id,
            started_at_ms: now,
            last_activity_at_ms: now,
            client_id,
            agent_id,
            agent_profile,
        };
        guard.insert(session_id.to_string(), run.clone());
        Ok(run)
    }

    pub async fn touch(&self, session_id: &str, run_id: &str) {
        let mut guard = self.active.write().await;
        if let Some(run) = guard.get_mut(session_id) {
            if run.run_id == run_id {
                run.last_activity_at_ms = now_ms();
            }
        }
    }

    pub async fn finish_if_match(&self, session_id: &str, run_id: &str) -> Option<ActiveRun> {
        let mut guard = self.active.write().await;
        if let Some(run) = guard.get(session_id) {
            if run.run_id == run_id {
                return guard.remove(session_id);
            }
        }
        None
    }

    pub async fn finish_active(&self, session_id: &str) -> Option<ActiveRun> {
        self.active.write().await.remove(session_id)
    }

    pub async fn reap_stale(&self, stale_ms: u64) -> Vec<(String, ActiveRun)> {
        let now = now_ms();
        let mut guard = self.active.write().await;
        let stale_ids = guard
            .iter()
            .filter_map(|(session_id, run)| {
                if now.saturating_sub(run.last_activity_at_ms) > stale_ms {
                    Some(session_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let mut out = Vec::with_capacity(stale_ids.len());
        for session_id in stale_ids {
            if let Some(run) = guard.remove(&session_id) {
                out.push((session_id, run));
            }
        }
        out
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn build_id() -> String {
    if let Some(explicit) = option_env!("TANDEM_BUILD_ID") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(git_sha) = option_env!("VERGEN_GIT_SHA") {
        let trimmed = git_sha.trim();
        if !trimmed.is_empty() {
            return format!("{}+{}", env!("CARGO_PKG_VERSION"), trimmed);
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

pub fn detect_host_runtime_context() -> HostRuntimeContext {
    let os = if cfg!(target_os = "windows") {
        HostOs::Windows
    } else if cfg!(target_os = "macos") {
        HostOs::Macos
    } else {
        HostOs::Linux
    };
    let (shell_family, path_style) = match os {
        HostOs::Windows => (ShellFamily::Powershell, PathStyle::Windows),
        HostOs::Linux | HostOs::Macos => (ShellFamily::Posix, PathStyle::Posix),
    };
    HostRuntimeContext {
        os,
        arch: std::env::consts::ARCH.to_string(),
        shell_family,
        path_style,
    }
}

pub fn binary_path_for_health() -> Option<String> {
    #[cfg(debug_assertions)]
    {
        std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

#[derive(Clone)]
pub struct RuntimeState {
    pub storage: Arc<Storage>,
    pub config: ConfigStore,
    pub event_bus: EventBus,
    pub providers: ProviderRegistry,
    pub plugins: PluginRegistry,
    pub agents: AgentRegistry,
    pub tools: ToolRegistry,
    pub permissions: PermissionManager,
    pub mcp: McpRegistry,
    pub pty: PtyManager,
    pub lsp: LspManager,
    pub auth: Arc<RwLock<std::collections::HashMap<String, String>>>,
    pub logs: Arc<RwLock<Vec<Value>>>,
    pub workspace_index: WorkspaceIndex,
    pub cancellations: CancellationRegistry,
    pub engine_loop: EngineLoop,
    pub host_runtime_context: HostRuntimeContext,
    pub browser: BrowserSubsystem,
}

#[derive(Debug, Clone)]
pub struct GovernedMemoryRecord {
    pub id: String,
    pub run_id: String,
    pub partition: MemoryPartition,
    pub kind: MemoryContentKind,
    pub content: String,
    pub artifact_refs: Vec<String>,
    pub classification: MemoryClassification,
    pub metadata: Option<Value>,
    pub source_memory_id: Option<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryAuditEvent {
    pub audit_id: String,
    pub action: String,
    pub run_id: String,
    pub memory_id: Option<String>,
    pub source_memory_id: Option<String>,
    pub to_tier: Option<GovernedMemoryTier>,
    pub partition_key: String,
    pub actor: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedResourceRecord {
    pub key: String,
    pub value: Value,
    pub rev: u64,
    pub updated_at_ms: u64,
    pub updated_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineSchedule {
    IntervalSeconds { seconds: u64 },
    Cron { expression: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RoutineMisfirePolicy {
    Skip,
    RunOnce,
    CatchUp { max_runs: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineStatus {
    Active,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineSpec {
    pub routine_id: String,
    pub name: String,
    pub status: RoutineStatus,
    pub schedule: RoutineSchedule,
    pub timezone: String,
    pub misfire_policy: RoutineMisfirePolicy,
    pub entrypoint: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub output_targets: Vec<String>,
    pub creator_type: String,
    pub creator_id: String,
    pub requires_approval: bool,
    pub external_integrations_allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineHistoryEvent {
    pub routine_id: String,
    pub trigger_type: String,
    pub run_count: u32,
    pub fired_at_ms: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineRunStatus {
    Queued,
    PendingApproval,
    Running,
    Paused,
    BlockedPolicy,
    Denied,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRunArtifact {
    pub artifact_id: String,
    pub uri: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRunRecord {
    pub run_id: String,
    pub routine_id: String,
    pub trigger_type: String,
    pub run_count: u32,
    pub status: RoutineRunStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fired_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    pub requires_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub entrypoint: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub output_targets: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<RoutineRunArtifact>,
    #[serde(default)]
    pub active_session_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_session_id: Option<String>,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone)]
pub struct RoutineSessionPolicy {
    pub session_id: String,
    pub run_id: String,
    pub routine_id: String,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoutineTriggerPlan {
    pub routine_id: String,
    pub run_count: u32,
    pub scheduled_at_ms: u64,
    pub next_fire_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationV2Status {
    Active,
    Paused,
    Draft,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationV2ScheduleType {
    Cron,
    Interval,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationV2Schedule {
    #[serde(rename = "type")]
    pub schedule_type: AutomationV2ScheduleType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_expression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_seconds: Option<u64>,
    pub timezone: String,
    pub misfire_policy: RoutineMisfirePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAgentToolPolicy {
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub denylist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAgentMcpPolicy {
    #[serde(default)]
    pub allowed_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAgentProfile {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default)]
    pub skills: Vec<String>,
    pub tool_policy: AutomationAgentToolPolicy,
    pub mcp_policy: AutomationAgentMcpPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFlowNode {
    pub node_id: String,
    pub agent_id: String,
    pub objective: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input_refs: Vec<AutomationFlowInputRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<AutomationFlowOutputContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFlowInputRef {
    pub from_step_id: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFlowOutputContract {
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFlowSpec {
    #[serde(default)]
    pub nodes: Vec<AutomationFlowNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationExecutionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel_agents: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_runtime_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_tool_calls: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationV2Spec {
    pub automation_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: AutomationV2Status,
    pub schedule: AutomationV2Schedule,
    #[serde(default)]
    pub agents: Vec<AutomationAgentProfile>,
    pub flow: AutomationFlowSpec,
    pub execution: AutomationExecutionPolicy,
    #[serde(default)]
    pub output_targets: Vec<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub creator_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlanStep {
    pub step_id: String,
    pub kind: String,
    pub objective: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub agent_role: String,
    #[serde(default)]
    pub input_refs: Vec<AutomationFlowInputRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<AutomationFlowOutputContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub plan_id: String,
    pub planner_version: String,
    pub plan_source: String,
    pub original_prompt: String,
    pub normalized_prompt: String,
    pub confidence: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub schedule: AutomationV2Schedule,
    pub execution_target: String,
    pub workspace_root: String,
    #[serde(default)]
    pub steps: Vec<WorkflowPlanStep>,
    #[serde(default)]
    pub requires_integrations: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_preferences: Option<Value>,
    pub save_options: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlanChatMessage {
    pub role: String,
    pub text: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlanConversation {
    pub conversation_id: String,
    pub plan_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub messages: Vec<WorkflowPlanChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlanDraftRecord {
    pub initial_plan: WorkflowPlan,
    pub current_plan: WorkflowPlan,
    pub conversation: WorkflowPlanConversation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_diagnostics: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationNodeOutput {
    pub contract_kind: String,
    pub summary: String,
    pub content: Value,
    pub created_at_ms: u64,
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    Queued,
    Running,
    Pausing,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRunCheckpoint {
    #[serde(default)]
    pub completed_nodes: Vec<String>,
    #[serde(default)]
    pub pending_nodes: Vec<String>,
    #[serde(default)]
    pub node_outputs: std::collections::HashMap<String, Value>,
    #[serde(default)]
    pub node_attempts: std::collections::HashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationV2RunRecord {
    pub run_id: String,
    pub automation_id: String,
    pub trigger_type: String,
    pub status: AutomationRunStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default)]
    pub active_session_ids: Vec<String>,
    #[serde(default)]
    pub active_instance_ids: Vec<String>,
    pub checkpoint: AutomationRunCheckpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_snapshot: Option<AutomationV2Spec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorProviderPreference {
    Auto,
    OfficialGithub,
    Composio,
    Arcade,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorLabelMode {
    ReporterOnly,
}

impl Default for BugMonitorLabelMode {
    fn default() -> Self {
        Self::ReporterOnly
    }
}

impl Default for BugMonitorProviderPreference {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugMonitorConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<String>,
    #[serde(default)]
    pub provider_preference: BugMonitorProviderPreference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default = "default_true")]
    pub auto_create_new_issues: bool,
    #[serde(default)]
    pub require_approval_for_new_issues: bool,
    #[serde(default = "default_true")]
    pub auto_comment_on_matched_open_issues: bool,
    #[serde(default)]
    pub label_mode: BugMonitorLabelMode,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl Default for BugMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            paused: false,
            workspace_root: None,
            repo: None,
            mcp_server: None,
            provider_preference: BugMonitorProviderPreference::Auto,
            model_policy: None,
            auto_create_new_issues: true,
            require_approval_for_new_issues: false,
            auto_comment_on_matched_open_issues: true,
            label_mode: BugMonitorLabelMode::ReporterOnly,
            updated_at_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorDraftRecord {
    pub draft_id: String,
    pub fingerprint: String,
    pub repo: String,
    pub status: String,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_issue_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_comment_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_posted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_issue_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_issue_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorPostRecord {
    pub post_id: String,
    pub draft_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    pub fingerprint: String,
    pub repo: String,
    pub operation: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorIncidentRecord {
    pub incident_id: String,
    pub fingerprint: String,
    pub event_type: String,
    pub status: String,
    pub repo: String,
    pub workspace_root: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub excerpt: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default)]
    pub occurrence_count: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_summary: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_matches: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorRuntimeStatus {
    #[serde(default)]
    pub monitoring_active: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub pending_incidents: usize,
    #[serde(default)]
    pub total_incidents: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_processed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_incident_event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_runtime_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post_result: Option<String>,
    #[serde(default)]
    pub pending_posts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorSubmission {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default)]
    pub excerpt: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorCapabilityReadiness {
    #[serde(default)]
    pub github_list_issues: bool,
    #[serde(default)]
    pub github_get_issue: bool,
    #[serde(default)]
    pub github_create_issue: bool,
    #[serde(default)]
    pub github_comment_on_issue: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorCapabilityMatch {
    pub capability_id: String,
    pub provider: String,
    pub tool_name: String,
    pub binding_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorBindingCandidate {
    pub capability_id: String,
    pub binding_tool_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorReadiness {
    #[serde(default)]
    pub config_valid: bool,
    #[serde(default)]
    pub repo_valid: bool,
    #[serde(default)]
    pub mcp_server_present: bool,
    #[serde(default)]
    pub mcp_connected: bool,
    #[serde(default)]
    pub github_read_ready: bool,
    #[serde(default)]
    pub github_write_ready: bool,
    #[serde(default)]
    pub selected_model_ready: bool,
    #[serde(default)]
    pub ingest_ready: bool,
    #[serde(default)]
    pub publish_ready: bool,
    #[serde(default)]
    pub runtime_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorStatus {
    pub config: BugMonitorConfig,
    pub readiness: BugMonitorReadiness,
    #[serde(default)]
    pub runtime: BugMonitorRuntimeStatus,
    pub required_capabilities: BugMonitorCapabilityReadiness,
    #[serde(default)]
    pub missing_required_capabilities: Vec<String>,
    #[serde(default)]
    pub resolved_capabilities: Vec<BugMonitorCapabilityMatch>,
    #[serde(default)]
    pub discovered_mcp_tools: Vec<String>,
    #[serde(default)]
    pub selected_server_binding_candidates: Vec<BugMonitorBindingCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_source_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bindings_last_merged_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<ModelSpec>,
    #[serde(default)]
    pub pending_drafts: usize,
    #[serde(default)]
    pub pending_posts: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceConflict {
    pub key: String,
    pub expected_rev: Option<u64>,
    pub current_rev: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceStoreError {
    InvalidKey { key: String },
    RevisionConflict(ResourceConflict),
    PersistFailed { message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoutineStoreError {
    InvalidRoutineId { routine_id: String },
    InvalidSchedule { detail: String },
    PersistFailed { message: String },
}

#[derive(Debug, Clone)]
pub enum StartupStatus {
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone)]
pub struct StartupState {
    pub status: StartupStatus,
    pub phase: String,
    pub started_at_ms: u64,
    pub attempt_id: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StartupSnapshot {
    pub status: StartupStatus,
    pub phase: String,
    pub started_at_ms: u64,
    pub attempt_id: String,
    pub last_error: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub runtime: Arc<OnceLock<RuntimeState>>,
    pub startup: Arc<RwLock<StartupState>>,
    pub in_process_mode: Arc<AtomicBool>,
    pub api_token: Arc<RwLock<Option<String>>>,
    pub engine_leases: Arc<RwLock<std::collections::HashMap<String, EngineLease>>>,
    pub run_registry: RunRegistry,
    pub run_stale_ms: u64,
    pub memory_records: Arc<RwLock<std::collections::HashMap<String, GovernedMemoryRecord>>>,
    pub memory_audit_log: Arc<RwLock<Vec<MemoryAuditEvent>>>,
    pub missions: Arc<RwLock<std::collections::HashMap<String, MissionState>>>,
    pub shared_resources: Arc<RwLock<std::collections::HashMap<String, SharedResourceRecord>>>,
    pub shared_resources_path: PathBuf,
    pub routines: Arc<RwLock<std::collections::HashMap<String, RoutineSpec>>>,
    pub routine_history: Arc<RwLock<std::collections::HashMap<String, Vec<RoutineHistoryEvent>>>>,
    pub routine_runs: Arc<RwLock<std::collections::HashMap<String, RoutineRunRecord>>>,
    pub automations_v2: Arc<RwLock<std::collections::HashMap<String, AutomationV2Spec>>>,
    pub automation_v2_runs: Arc<RwLock<std::collections::HashMap<String, AutomationV2RunRecord>>>,
    pub workflow_plans: Arc<RwLock<std::collections::HashMap<String, WorkflowPlan>>>,
    pub workflow_plan_drafts:
        Arc<RwLock<std::collections::HashMap<String, WorkflowPlanDraftRecord>>>,
    pub bug_monitor_config: Arc<RwLock<BugMonitorConfig>>,
    pub bug_monitor_drafts: Arc<RwLock<std::collections::HashMap<String, BugMonitorDraftRecord>>>,
    pub bug_monitor_incidents:
        Arc<RwLock<std::collections::HashMap<String, BugMonitorIncidentRecord>>>,
    pub bug_monitor_posts: Arc<RwLock<std::collections::HashMap<String, BugMonitorPostRecord>>>,
    pub bug_monitor_runtime_status: Arc<RwLock<BugMonitorRuntimeStatus>>,
    pub workflows: Arc<RwLock<WorkflowRegistry>>,
    pub workflow_runs: Arc<RwLock<std::collections::HashMap<String, WorkflowRunRecord>>>,
    pub workflow_hook_overrides: Arc<RwLock<std::collections::HashMap<String, bool>>>,
    pub workflow_dispatch_seen: Arc<RwLock<std::collections::HashMap<String, u64>>>,
    pub routine_session_policies:
        Arc<RwLock<std::collections::HashMap<String, RoutineSessionPolicy>>>,
    pub automation_v2_session_runs: Arc<RwLock<std::collections::HashMap<String, String>>>,
    pub token_cost_per_1k_usd: f64,
    pub routines_path: PathBuf,
    pub routine_history_path: PathBuf,
    pub routine_runs_path: PathBuf,
    pub automations_v2_path: PathBuf,
    pub automation_v2_runs_path: PathBuf,
    pub bug_monitor_config_path: PathBuf,
    pub bug_monitor_drafts_path: PathBuf,
    pub bug_monitor_incidents_path: PathBuf,
    pub bug_monitor_posts_path: PathBuf,
    pub workflow_runs_path: PathBuf,
    pub workflow_hook_overrides_path: PathBuf,
    pub agent_teams: AgentTeamRuntime,
    pub web_ui_enabled: Arc<AtomicBool>,
    pub web_ui_prefix: Arc<std::sync::RwLock<String>>,
    pub server_base_url: Arc<std::sync::RwLock<String>>,
    pub channels_runtime: Arc<tokio::sync::Mutex<ChannelRuntime>>,
    pub host_runtime_context: HostRuntimeContext,
    pub pack_manager: Arc<PackManager>,
    pub capability_resolver: Arc<CapabilityResolver>,
    pub preset_registry: Arc<PresetRegistry>,
}

#[derive(Debug, Clone)]
struct StatusIndexUpdate {
    key: String,
    value: Value,
}

impl AppState {
    pub fn new_starting(attempt_id: String, in_process: bool) -> Self {
        Self {
            runtime: Arc::new(OnceLock::new()),
            startup: Arc::new(RwLock::new(StartupState {
                status: StartupStatus::Starting,
                phase: "boot".to_string(),
                started_at_ms: now_ms(),
                attempt_id,
                last_error: None,
            })),
            in_process_mode: Arc::new(AtomicBool::new(in_process)),
            api_token: Arc::new(RwLock::new(None)),
            engine_leases: Arc::new(RwLock::new(std::collections::HashMap::new())),
            run_registry: RunRegistry::new(),
            run_stale_ms: resolve_run_stale_ms(),
            memory_records: Arc::new(RwLock::new(std::collections::HashMap::new())),
            memory_audit_log: Arc::new(RwLock::new(Vec::new())),
            missions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            shared_resources: Arc::new(RwLock::new(std::collections::HashMap::new())),
            shared_resources_path: resolve_shared_resources_path(),
            routines: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_history: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automations_v2: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automation_v2_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_plans: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_plan_drafts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_config: Arc::new(RwLock::new(resolve_bug_monitor_env_config())),
            bug_monitor_drafts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_incidents: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_posts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_runtime_status: Arc::new(RwLock::new(BugMonitorRuntimeStatus::default())),
            workflows: Arc::new(RwLock::new(WorkflowRegistry::default())),
            workflow_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_hook_overrides: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_dispatch_seen: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_session_policies: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automation_v2_session_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routines_path: resolve_routines_path(),
            routine_history_path: resolve_routine_history_path(),
            routine_runs_path: resolve_routine_runs_path(),
            automations_v2_path: resolve_automations_v2_path(),
            automation_v2_runs_path: resolve_automation_v2_runs_path(),
            bug_monitor_config_path: resolve_bug_monitor_config_path(),
            bug_monitor_drafts_path: resolve_bug_monitor_drafts_path(),
            bug_monitor_incidents_path: resolve_bug_monitor_incidents_path(),
            bug_monitor_posts_path: resolve_bug_monitor_posts_path(),
            workflow_runs_path: resolve_workflow_runs_path(),
            workflow_hook_overrides_path: resolve_workflow_hook_overrides_path(),
            agent_teams: AgentTeamRuntime::new(resolve_agent_team_audit_path()),
            web_ui_enabled: Arc::new(AtomicBool::new(false)),
            web_ui_prefix: Arc::new(std::sync::RwLock::new("/admin".to_string())),
            server_base_url: Arc::new(std::sync::RwLock::new("http://127.0.0.1:39731".to_string())),
            channels_runtime: Arc::new(tokio::sync::Mutex::new(ChannelRuntime::default())),
            host_runtime_context: detect_host_runtime_context(),
            token_cost_per_1k_usd: resolve_token_cost_per_1k_usd(),
            pack_manager: Arc::new(PackManager::new(PackManager::default_root())),
            capability_resolver: Arc::new(CapabilityResolver::new(PackManager::default_root())),
            preset_registry: Arc::new(PresetRegistry::new(
                PackManager::default_root(),
                resolve_shared_paths()
                    .map(|paths| paths.canonical_root)
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("."))
                            .join(".tandem")
                    }),
            )),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.runtime.get().is_some()
    }

    pub async fn wait_until_ready_or_failed(&self, attempts: usize, sleep_ms: u64) -> bool {
        for _ in 0..attempts {
            if self.is_ready() {
                return true;
            }
            let startup = self.startup_snapshot().await;
            if matches!(startup.status, StartupStatus::Failed) {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
        }
        self.is_ready()
    }

    pub fn mode_label(&self) -> &'static str {
        if self.in_process_mode.load(Ordering::Relaxed) {
            "in-process"
        } else {
            "sidecar"
        }
    }

    pub fn configure_web_ui(&self, enabled: bool, prefix: String) {
        self.web_ui_enabled.store(enabled, Ordering::Relaxed);
        if let Ok(mut guard) = self.web_ui_prefix.write() {
            *guard = normalize_web_ui_prefix(&prefix);
        }
    }

    pub fn web_ui_enabled(&self) -> bool {
        self.web_ui_enabled.load(Ordering::Relaxed)
    }

    pub fn web_ui_prefix(&self) -> String {
        self.web_ui_prefix
            .read()
            .map(|v| v.clone())
            .unwrap_or_else(|_| "/admin".to_string())
    }

    pub fn set_server_base_url(&self, base_url: String) {
        if let Ok(mut guard) = self.server_base_url.write() {
            *guard = base_url;
        }
    }

    pub fn server_base_url(&self) -> String {
        self.server_base_url
            .read()
            .map(|v| v.clone())
            .unwrap_or_else(|_| "http://127.0.0.1:39731".to_string())
    }

    pub async fn api_token(&self) -> Option<String> {
        self.api_token.read().await.clone()
    }

    pub async fn set_api_token(&self, token: Option<String>) {
        *self.api_token.write().await = token;
    }

    pub async fn startup_snapshot(&self) -> StartupSnapshot {
        let state = self.startup.read().await.clone();
        StartupSnapshot {
            elapsed_ms: now_ms().saturating_sub(state.started_at_ms),
            status: state.status,
            phase: state.phase,
            started_at_ms: state.started_at_ms,
            attempt_id: state.attempt_id,
            last_error: state.last_error,
        }
    }

    pub fn host_runtime_context(&self) -> HostRuntimeContext {
        self.runtime
            .get()
            .map(|runtime| runtime.host_runtime_context.clone())
            .unwrap_or_else(|| self.host_runtime_context.clone())
    }

    pub async fn set_phase(&self, phase: impl Into<String>) {
        let mut startup = self.startup.write().await;
        startup.phase = phase.into();
    }

    pub async fn mark_ready(&self, runtime: RuntimeState) -> anyhow::Result<()> {
        self.runtime
            .set(runtime)
            .map_err(|_| anyhow::anyhow!("runtime already initialized"))?;
        self.register_browser_tools().await?;
        self.tools
            .register_tool(
                "pack_builder".to_string(),
                Arc::new(crate::pack_builder::PackBuilderTool::new(self.clone())),
            )
            .await;
        self.engine_loop
            .set_spawn_agent_hook(std::sync::Arc::new(
                crate::agent_teams::ServerSpawnAgentHook::new(self.clone()),
            ))
            .await;
        self.engine_loop
            .set_tool_policy_hook(std::sync::Arc::new(
                crate::agent_teams::ServerToolPolicyHook::new(self.clone()),
            ))
            .await;
        self.engine_loop
            .set_prompt_context_hook(std::sync::Arc::new(ServerPromptContextHook::new(
                self.clone(),
            )))
            .await;
        let _ = self.load_shared_resources().await;
        self.load_routines().await?;
        let _ = self.load_routine_history().await;
        let _ = self.load_routine_runs().await;
        self.load_automations_v2().await?;
        let _ = self.load_automation_v2_runs().await;
        let _ = self.load_bug_monitor_config().await;
        let _ = self.load_bug_monitor_drafts().await;
        let _ = self.load_bug_monitor_incidents().await;
        let _ = self.load_bug_monitor_posts().await;
        let _ = self.load_workflow_runs().await;
        let _ = self.load_workflow_hook_overrides().await;
        let _ = self.reload_workflows().await;
        let workspace_root = self.workspace_index.snapshot().await.root;
        let _ = self
            .agent_teams
            .ensure_loaded_for_workspace(&workspace_root)
            .await;
        let mut startup = self.startup.write().await;
        startup.status = StartupStatus::Ready;
        startup.phase = "ready".to_string();
        startup.last_error = None;
        Ok(())
    }

    pub async fn mark_failed(&self, phase: impl Into<String>, error: impl Into<String>) {
        let mut startup = self.startup.write().await;
        startup.status = StartupStatus::Failed;
        startup.phase = phase.into();
        startup.last_error = Some(error.into());
    }

    pub async fn channel_statuses(&self) -> std::collections::HashMap<String, ChannelStatus> {
        let runtime = self.channels_runtime.lock().await;
        runtime.statuses.clone()
    }

    pub async fn restart_channel_listeners(&self) -> anyhow::Result<()> {
        let effective = self.config.get_effective_value().await;
        let parsed: EffectiveAppConfig = serde_json::from_value(effective).unwrap_or_default();
        self.configure_web_ui(parsed.web_ui.enabled, parsed.web_ui.path_prefix.clone());

        let mut runtime = self.channels_runtime.lock().await;
        if let Some(listeners) = runtime.listeners.as_mut() {
            listeners.abort_all();
        }
        runtime.listeners = None;
        runtime.statuses.clear();

        let mut status_map = std::collections::HashMap::new();
        status_map.insert(
            "telegram".to_string(),
            ChannelStatus {
                enabled: parsed.channels.telegram.is_some(),
                connected: false,
                last_error: None,
                active_sessions: 0,
                meta: serde_json::json!({}),
            },
        );
        status_map.insert(
            "discord".to_string(),
            ChannelStatus {
                enabled: parsed.channels.discord.is_some(),
                connected: false,
                last_error: None,
                active_sessions: 0,
                meta: serde_json::json!({}),
            },
        );
        status_map.insert(
            "slack".to_string(),
            ChannelStatus {
                enabled: parsed.channels.slack.is_some(),
                connected: false,
                last_error: None,
                active_sessions: 0,
                meta: serde_json::json!({}),
            },
        );

        if let Some(channels_cfg) = build_channels_config(self, &parsed.channels).await {
            let listeners = tandem_channels::start_channel_listeners(channels_cfg).await;
            runtime.listeners = Some(listeners);
            for status in status_map.values_mut() {
                if status.enabled {
                    status.connected = true;
                }
            }
        }

        runtime.statuses = status_map.clone();
        drop(runtime);

        self.event_bus.publish(EngineEvent::new(
            "channel.status.changed",
            serde_json::json!({ "channels": status_map }),
        ));
        Ok(())
    }

    pub async fn load_shared_resources(&self) -> anyhow::Result<()> {
        if !self.shared_resources_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.shared_resources_path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, SharedResourceRecord>>(&raw)
                .unwrap_or_default();
        let mut guard = self.shared_resources.write().await;
        *guard = parsed;
        Ok(())
    }

    pub async fn persist_shared_resources(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.shared_resources_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.shared_resources.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.shared_resources_path, payload).await?;
        Ok(())
    }

    pub async fn get_shared_resource(&self, key: &str) -> Option<SharedResourceRecord> {
        self.shared_resources.read().await.get(key).cloned()
    }

    pub async fn list_shared_resources(
        &self,
        prefix: Option<&str>,
        limit: usize,
    ) -> Vec<SharedResourceRecord> {
        let limit = limit.clamp(1, 500);
        let mut rows = self
            .shared_resources
            .read()
            .await
            .values()
            .filter(|record| {
                if let Some(prefix) = prefix {
                    record.key.starts_with(prefix)
                } else {
                    true
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        rows.truncate(limit);
        rows
    }

    pub async fn put_shared_resource(
        &self,
        key: String,
        value: Value,
        if_match_rev: Option<u64>,
        updated_by: String,
        ttl_ms: Option<u64>,
    ) -> Result<SharedResourceRecord, ResourceStoreError> {
        if !is_valid_resource_key(&key) {
            return Err(ResourceStoreError::InvalidKey { key });
        }

        let now = now_ms();
        let mut guard = self.shared_resources.write().await;
        let existing = guard.get(&key).cloned();

        if let Some(expected) = if_match_rev {
            let current = existing.as_ref().map(|row| row.rev);
            if current != Some(expected) {
                return Err(ResourceStoreError::RevisionConflict(ResourceConflict {
                    key,
                    expected_rev: Some(expected),
                    current_rev: current,
                }));
            }
        }

        let next_rev = existing
            .as_ref()
            .map(|row| row.rev.saturating_add(1))
            .unwrap_or(1);

        let record = SharedResourceRecord {
            key: key.clone(),
            value,
            rev: next_rev,
            updated_at_ms: now,
            updated_by,
            ttl_ms,
        };

        let previous = guard.insert(key.clone(), record.clone());
        drop(guard);

        if let Err(error) = self.persist_shared_resources().await {
            let mut rollback = self.shared_resources.write().await;
            if let Some(previous) = previous {
                rollback.insert(key, previous);
            } else {
                rollback.remove(&key);
            }
            return Err(ResourceStoreError::PersistFailed {
                message: error.to_string(),
            });
        }

        Ok(record)
    }

    pub async fn delete_shared_resource(
        &self,
        key: &str,
        if_match_rev: Option<u64>,
    ) -> Result<Option<SharedResourceRecord>, ResourceStoreError> {
        if !is_valid_resource_key(key) {
            return Err(ResourceStoreError::InvalidKey {
                key: key.to_string(),
            });
        }

        let mut guard = self.shared_resources.write().await;
        let current = guard.get(key).cloned();
        if let Some(expected) = if_match_rev {
            let current_rev = current.as_ref().map(|row| row.rev);
            if current_rev != Some(expected) {
                return Err(ResourceStoreError::RevisionConflict(ResourceConflict {
                    key: key.to_string(),
                    expected_rev: Some(expected),
                    current_rev,
                }));
            }
        }

        let removed = guard.remove(key);
        drop(guard);

        if let Err(error) = self.persist_shared_resources().await {
            if let Some(record) = removed.clone() {
                self.shared_resources
                    .write()
                    .await
                    .insert(record.key.clone(), record);
            }
            return Err(ResourceStoreError::PersistFailed {
                message: error.to_string(),
            });
        }

        Ok(removed)
    }

    pub async fn load_routines(&self) -> anyhow::Result<()> {
        if !self.routines_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.routines_path).await?;
        match serde_json::from_str::<std::collections::HashMap<String, RoutineSpec>>(&raw) {
            Ok(parsed) => {
                let mut guard = self.routines.write().await;
                *guard = parsed;
                Ok(())
            }
            Err(primary_err) => {
                let backup_path = sibling_backup_path(&self.routines_path);
                if backup_path.exists() {
                    let backup_raw = fs::read_to_string(&backup_path).await?;
                    if let Ok(parsed_backup) = serde_json::from_str::<
                        std::collections::HashMap<String, RoutineSpec>,
                    >(&backup_raw)
                    {
                        let mut guard = self.routines.write().await;
                        *guard = parsed_backup;
                        return Ok(());
                    }
                }
                Err(anyhow::anyhow!(
                    "failed to parse routines store {}: {primary_err}",
                    self.routines_path.display()
                ))
            }
        }
    }

    pub async fn load_routine_history(&self) -> anyhow::Result<()> {
        if !self.routine_history_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.routine_history_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, Vec<RoutineHistoryEvent>>,
        >(&raw)
        .unwrap_or_default();
        let mut guard = self.routine_history.write().await;
        *guard = parsed;
        Ok(())
    }

    pub async fn load_routine_runs(&self) -> anyhow::Result<()> {
        if !self.routine_runs_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.routine_runs_path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, RoutineRunRecord>>(&raw)
                .unwrap_or_default();
        let mut guard = self.routine_runs.write().await;
        *guard = parsed;
        Ok(())
    }

    async fn persist_routines_inner(&self, allow_empty_overwrite: bool) -> anyhow::Result<()> {
        if let Some(parent) = self.routines_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let (payload, is_empty) = {
            let guard = self.routines.read().await;
            (serde_json::to_string_pretty(&*guard)?, guard.is_empty())
        };
        if is_empty && !allow_empty_overwrite && self.routines_path.exists() {
            let existing_raw = fs::read_to_string(&self.routines_path)
                .await
                .unwrap_or_default();
            let existing_has_rows = serde_json::from_str::<
                std::collections::HashMap<String, RoutineSpec>,
            >(&existing_raw)
            .map(|rows| !rows.is_empty())
            .unwrap_or(true);
            if existing_has_rows {
                return Err(anyhow::anyhow!(
                    "refusing to overwrite non-empty routines store {} with empty in-memory state",
                    self.routines_path.display()
                ));
            }
        }
        let backup_path = sibling_backup_path(&self.routines_path);
        if self.routines_path.exists() {
            let _ = fs::copy(&self.routines_path, &backup_path).await;
        }
        let tmp_path = sibling_tmp_path(&self.routines_path);
        fs::write(&tmp_path, payload).await?;
        fs::rename(&tmp_path, &self.routines_path).await?;
        Ok(())
    }

    pub async fn persist_routines(&self) -> anyhow::Result<()> {
        self.persist_routines_inner(false).await
    }

    pub async fn persist_routine_history(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.routine_history_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.routine_history.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.routine_history_path, payload).await?;
        Ok(())
    }

    pub async fn persist_routine_runs(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.routine_runs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.routine_runs.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.routine_runs_path, payload).await?;
        Ok(())
    }

    pub async fn put_routine(
        &self,
        mut routine: RoutineSpec,
    ) -> Result<RoutineSpec, RoutineStoreError> {
        if routine.routine_id.trim().is_empty() {
            return Err(RoutineStoreError::InvalidRoutineId {
                routine_id: routine.routine_id,
            });
        }

        routine.allowed_tools = normalize_allowed_tools(routine.allowed_tools);
        routine.output_targets = normalize_non_empty_list(routine.output_targets);

        let now = now_ms();
        let next_schedule_fire =
            compute_next_schedule_fire_at_ms(&routine.schedule, &routine.timezone, now)
                .ok_or_else(|| RoutineStoreError::InvalidSchedule {
                    detail: "invalid schedule or timezone".to_string(),
                })?;
        match routine.schedule {
            RoutineSchedule::IntervalSeconds { seconds } => {
                if seconds == 0 {
                    return Err(RoutineStoreError::InvalidSchedule {
                        detail: "interval_seconds must be > 0".to_string(),
                    });
                }
                let _ = seconds;
            }
            RoutineSchedule::Cron { .. } => {}
        }
        if routine.next_fire_at_ms.is_none() {
            routine.next_fire_at_ms = Some(next_schedule_fire);
        }

        let mut guard = self.routines.write().await;
        let previous = guard.insert(routine.routine_id.clone(), routine.clone());
        drop(guard);

        if let Err(error) = self.persist_routines().await {
            let mut rollback = self.routines.write().await;
            if let Some(previous) = previous {
                rollback.insert(previous.routine_id.clone(), previous);
            } else {
                rollback.remove(&routine.routine_id);
            }
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }

        Ok(routine)
    }

    pub async fn list_routines(&self) -> Vec<RoutineSpec> {
        let mut rows = self
            .routines
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.routine_id.cmp(&b.routine_id));
        rows
    }

    pub async fn get_routine(&self, routine_id: &str) -> Option<RoutineSpec> {
        self.routines.read().await.get(routine_id).cloned()
    }

    pub async fn delete_routine(
        &self,
        routine_id: &str,
    ) -> Result<Option<RoutineSpec>, RoutineStoreError> {
        let mut guard = self.routines.write().await;
        let removed = guard.remove(routine_id);
        drop(guard);

        let allow_empty_overwrite = self.routines.read().await.is_empty();
        if let Err(error) = self.persist_routines_inner(allow_empty_overwrite).await {
            if let Some(removed) = removed.clone() {
                self.routines
                    .write()
                    .await
                    .insert(removed.routine_id.clone(), removed);
            }
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }
        Ok(removed)
    }

    pub async fn evaluate_routine_misfires(&self, now_ms: u64) -> Vec<RoutineTriggerPlan> {
        let mut plans = Vec::new();
        let mut guard = self.routines.write().await;
        for routine in guard.values_mut() {
            if routine.status != RoutineStatus::Active {
                continue;
            }
            let Some(next_fire_at_ms) = routine.next_fire_at_ms else {
                continue;
            };
            if now_ms < next_fire_at_ms {
                continue;
            }
            let (run_count, next_fire_at_ms) = compute_misfire_plan_for_schedule(
                now_ms,
                next_fire_at_ms,
                &routine.schedule,
                &routine.timezone,
                &routine.misfire_policy,
            );
            routine.next_fire_at_ms = Some(next_fire_at_ms);
            if run_count == 0 {
                continue;
            }
            plans.push(RoutineTriggerPlan {
                routine_id: routine.routine_id.clone(),
                run_count,
                scheduled_at_ms: now_ms,
                next_fire_at_ms,
            });
        }
        drop(guard);
        let _ = self.persist_routines().await;
        plans
    }

    pub async fn mark_routine_fired(
        &self,
        routine_id: &str,
        fired_at_ms: u64,
    ) -> Option<RoutineSpec> {
        let mut guard = self.routines.write().await;
        let routine = guard.get_mut(routine_id)?;
        routine.last_fired_at_ms = Some(fired_at_ms);
        let updated = routine.clone();
        drop(guard);
        let _ = self.persist_routines().await;
        Some(updated)
    }

    pub async fn append_routine_history(&self, event: RoutineHistoryEvent) {
        let mut history = self.routine_history.write().await;
        history
            .entry(event.routine_id.clone())
            .or_default()
            .push(event);
        drop(history);
        let _ = self.persist_routine_history().await;
    }

    pub async fn list_routine_history(
        &self,
        routine_id: &str,
        limit: usize,
    ) -> Vec<RoutineHistoryEvent> {
        let limit = limit.clamp(1, 500);
        let mut rows = self
            .routine_history
            .read()
            .await
            .get(routine_id)
            .cloned()
            .unwrap_or_default();
        rows.sort_by(|a, b| b.fired_at_ms.cmp(&a.fired_at_ms));
        rows.truncate(limit);
        rows
    }

    pub async fn create_routine_run(
        &self,
        routine: &RoutineSpec,
        trigger_type: &str,
        run_count: u32,
        status: RoutineRunStatus,
        detail: Option<String>,
    ) -> RoutineRunRecord {
        let now = now_ms();
        let record = RoutineRunRecord {
            run_id: format!("routine-run-{}", uuid::Uuid::new_v4()),
            routine_id: routine.routine_id.clone(),
            trigger_type: trigger_type.to_string(),
            run_count,
            status,
            created_at_ms: now,
            updated_at_ms: now,
            fired_at_ms: Some(now),
            started_at_ms: None,
            finished_at_ms: None,
            requires_approval: routine.requires_approval,
            approval_reason: None,
            denial_reason: None,
            paused_reason: None,
            detail,
            entrypoint: routine.entrypoint.clone(),
            args: routine.args.clone(),
            allowed_tools: routine.allowed_tools.clone(),
            output_targets: routine.output_targets.clone(),
            artifacts: Vec::new(),
            active_session_ids: Vec::new(),
            latest_session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        };
        self.routine_runs
            .write()
            .await
            .insert(record.run_id.clone(), record.clone());
        let _ = self.persist_routine_runs().await;
        record
    }

    pub async fn get_routine_run(&self, run_id: &str) -> Option<RoutineRunRecord> {
        self.routine_runs.read().await.get(run_id).cloned()
    }

    pub async fn list_routine_runs(
        &self,
        routine_id: Option<&str>,
        limit: usize,
    ) -> Vec<RoutineRunRecord> {
        let mut rows = self
            .routine_runs
            .read()
            .await
            .values()
            .filter(|row| {
                if let Some(id) = routine_id {
                    row.routine_id == id
                } else {
                    true
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn claim_next_queued_routine_run(&self) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let next_run_id = guard
            .values()
            .filter(|row| row.status == RoutineRunStatus::Queued)
            .min_by(|a, b| {
                a.created_at_ms
                    .cmp(&b.created_at_ms)
                    .then_with(|| a.run_id.cmp(&b.run_id))
            })
            .map(|row| row.run_id.clone())?;
        let now = now_ms();
        let row = guard.get_mut(&next_run_id)?;
        row.status = RoutineRunStatus::Running;
        row.updated_at_ms = now;
        row.started_at_ms = Some(now);
        let claimed = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(claimed)
    }

    pub async fn set_routine_session_policy(
        &self,
        session_id: String,
        run_id: String,
        routine_id: String,
        allowed_tools: Vec<String>,
    ) {
        let policy = RoutineSessionPolicy {
            session_id: session_id.clone(),
            run_id,
            routine_id,
            allowed_tools: normalize_allowed_tools(allowed_tools),
        };
        self.routine_session_policies
            .write()
            .await
            .insert(session_id, policy);
    }

    pub async fn routine_session_policy(&self, session_id: &str) -> Option<RoutineSessionPolicy> {
        self.routine_session_policies
            .read()
            .await
            .get(session_id)
            .cloned()
    }

    pub async fn clear_routine_session_policy(&self, session_id: &str) {
        self.routine_session_policies
            .write()
            .await
            .remove(session_id);
    }

    pub async fn update_routine_run_status(
        &self,
        run_id: &str,
        status: RoutineRunStatus,
        reason: Option<String>,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        row.status = status.clone();
        row.updated_at_ms = now_ms();
        match status {
            RoutineRunStatus::PendingApproval => row.approval_reason = reason,
            RoutineRunStatus::Running => {
                row.started_at_ms.get_or_insert_with(now_ms);
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
            RoutineRunStatus::Denied => row.denial_reason = reason,
            RoutineRunStatus::Paused => row.paused_reason = reason,
            RoutineRunStatus::Completed
            | RoutineRunStatus::Failed
            | RoutineRunStatus::Cancelled => {
                row.finished_at_ms = Some(now_ms());
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
            _ => {
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
        }
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn append_routine_run_artifact(
        &self,
        run_id: &str,
        artifact: RoutineRunArtifact,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        row.updated_at_ms = now_ms();
        row.artifacts.push(artifact);
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn add_active_session_id(
        &self,
        run_id: &str,
        session_id: String,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        if !row.active_session_ids.iter().any(|id| id == &session_id) {
            row.active_session_ids.push(session_id);
        }
        row.latest_session_id = row.active_session_ids.last().cloned();
        row.updated_at_ms = now_ms();
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn clear_active_session_id(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        row.active_session_ids.retain(|id| id != session_id);
        row.updated_at_ms = now_ms();
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn load_automations_v2(&self) -> anyhow::Result<()> {
        let mut merged = std::collections::HashMap::<String, AutomationV2Spec>::new();
        let mut loaded_from_alternate = false;
        let mut path_counts = Vec::new();
        for path in candidate_automations_v2_paths(&self.automations_v2_path) {
            if !path.exists() {
                path_counts.push((path, 0usize));
                continue;
            }
            let raw = fs::read_to_string(&path).await?;
            if raw.trim().is_empty() || raw.trim() == "{}" {
                path_counts.push((path, 0usize));
                continue;
            }
            let parsed = parse_automation_v2_file(&raw);
            path_counts.push((path.clone(), parsed.len()));
            if path != self.automations_v2_path {
                loaded_from_alternate = loaded_from_alternate || !parsed.is_empty();
            }
            for (automation_id, automation) in parsed {
                match merged.get(&automation_id) {
                    Some(existing) if existing.updated_at_ms > automation.updated_at_ms => {}
                    _ => {
                        merged.insert(automation_id, automation);
                    }
                }
            }
        }
        let active_path = self.automations_v2_path.display().to_string();
        let path_count_summary = path_counts
            .iter()
            .map(|(path, count)| format!("{}={count}", path.display()))
            .collect::<Vec<_>>();
        tracing::info!(
            active_path,
            path_counts = ?path_count_summary,
            merged_count = merged.len(),
            "loaded automation v2 definitions"
        );
        *self.automations_v2.write().await = merged;
        if loaded_from_alternate {
            let _ = self.persist_automations_v2().await;
        }
        Ok(())
    }

    pub async fn persist_automations_v2(&self) -> anyhow::Result<()> {
        let payload = {
            let guard = self.automations_v2.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.automations_v2_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.automations_v2_path, &payload).await?;
        Ok(())
    }

    pub async fn load_automation_v2_runs(&self) -> anyhow::Result<()> {
        let mut merged = std::collections::HashMap::<String, AutomationV2RunRecord>::new();
        let mut loaded_from_alternate = false;
        let mut path_counts = Vec::new();
        for path in candidate_automation_v2_runs_paths(&self.automation_v2_runs_path) {
            if !path.exists() {
                path_counts.push((path, 0usize));
                continue;
            }
            let raw = fs::read_to_string(&path).await?;
            if raw.trim().is_empty() || raw.trim() == "{}" {
                path_counts.push((path, 0usize));
                continue;
            }
            let parsed = parse_automation_v2_runs_file(&raw);
            path_counts.push((path.clone(), parsed.len()));
            if path != self.automation_v2_runs_path {
                loaded_from_alternate = loaded_from_alternate || !parsed.is_empty();
            }
            for (run_id, run) in parsed {
                match merged.get(&run_id) {
                    Some(existing) if existing.updated_at_ms > run.updated_at_ms => {}
                    _ => {
                        merged.insert(run_id, run);
                    }
                }
            }
        }
        let active_runs_path = self.automation_v2_runs_path.display().to_string();
        let run_path_count_summary = path_counts
            .iter()
            .map(|(path, count)| format!("{}={count}", path.display()))
            .collect::<Vec<_>>();
        tracing::info!(
            active_path = active_runs_path,
            path_counts = ?run_path_count_summary,
            merged_count = merged.len(),
            "loaded automation v2 runs"
        );
        *self.automation_v2_runs.write().await = merged;
        let recovered = self
            .recover_automation_definitions_from_run_snapshots()
            .await?;
        let automation_count = self.automations_v2.read().await.len();
        let run_count = self.automation_v2_runs.read().await.len();
        if automation_count == 0 && run_count > 0 {
            let active_automations_path = self.automations_v2_path.display().to_string();
            let active_runs_path = self.automation_v2_runs_path.display().to_string();
            tracing::warn!(
                active_automations_path,
                active_runs_path,
                run_count,
                "automation v2 definitions are empty while run history exists"
            );
        }
        if loaded_from_alternate || recovered > 0 {
            let _ = self.persist_automation_v2_runs().await;
        }
        Ok(())
    }

    pub async fn persist_automation_v2_runs(&self) -> anyhow::Result<()> {
        let payload = {
            let guard = self.automation_v2_runs.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.automation_v2_runs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.automation_v2_runs_path, &payload).await?;
        Ok(())
    }

    async fn verify_automation_v2_persisted(
        &self,
        automation_id: &str,
        expected_present: bool,
    ) -> anyhow::Result<()> {
        let candidate_paths = candidate_automations_v2_paths(&self.automations_v2_path);
        let mut mismatches = Vec::new();
        for path in candidate_paths {
            let raw = if path.exists() {
                fs::read_to_string(&path).await?
            } else {
                String::new()
            };
            let parsed = parse_automation_v2_file(&raw);
            let present = parsed.contains_key(automation_id);
            if present != expected_present {
                mismatches.push(format!(
                    "{} expected_present={} actual_present={} count={}",
                    path.display(),
                    expected_present,
                    present,
                    parsed.len()
                ));
            }
        }
        if !mismatches.is_empty() {
            let active_path = self.automations_v2_path.display().to_string();
            tracing::error!(
                automation_id,
                expected_present,
                mismatches = ?mismatches,
                active_path,
                "automation v2 persistence verification failed"
            );
            anyhow::bail!(
                "automation v2 persistence verification failed for `{}`",
                automation_id
            );
        }
        Ok(())
    }

    async fn recover_automation_definitions_from_run_snapshots(&self) -> anyhow::Result<usize> {
        let runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut guard = self.automations_v2.write().await;
        let mut recovered = 0usize;
        for run in runs {
            let Some(snapshot) = run.automation_snapshot.clone() else {
                continue;
            };
            let should_replace = match guard.get(&run.automation_id) {
                Some(existing) => existing.updated_at_ms < snapshot.updated_at_ms,
                None => true,
            };
            if should_replace {
                if !guard.contains_key(&run.automation_id) {
                    recovered += 1;
                }
                guard.insert(run.automation_id.clone(), snapshot);
            }
        }
        drop(guard);
        if recovered > 0 {
            let active_path = self.automations_v2_path.display().to_string();
            tracing::warn!(
                recovered,
                active_path,
                "recovered automation v2 definitions from run snapshots"
            );
            self.persist_automations_v2().await?;
        }
        Ok(recovered)
    }

    pub async fn load_bug_monitor_config(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_config_path.exists() {
            self.bug_monitor_config_path.clone()
        } else if legacy_failure_reporter_path("failure_reporter_config.json").exists() {
            legacy_failure_reporter_path("failure_reporter_config.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed = serde_json::from_str::<BugMonitorConfig>(&raw)
            .unwrap_or_else(|_| resolve_bug_monitor_env_config());
        *self.bug_monitor_config.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_config(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_config_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_config.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_config_path, payload).await?;
        Ok(())
    }

    pub async fn bug_monitor_config(&self) -> BugMonitorConfig {
        self.bug_monitor_config.read().await.clone()
    }

    pub async fn put_bug_monitor_config(
        &self,
        mut config: BugMonitorConfig,
    ) -> anyhow::Result<BugMonitorConfig> {
        config.workspace_root = config
            .workspace_root
            .as_ref()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        if let Some(repo) = config.repo.as_ref() {
            if !repo.is_empty() && !is_valid_owner_repo_slug(repo) {
                anyhow::bail!("repo must be in owner/repo format");
            }
        }
        if let Some(server) = config.mcp_server.as_ref() {
            let servers = self.mcp.list().await;
            if !servers.contains_key(server) {
                anyhow::bail!("unknown mcp server `{server}`");
            }
        }
        if let Some(model_policy) = config.model_policy.as_ref() {
            crate::http::routines_automations::validate_model_policy(model_policy)
                .map_err(anyhow::Error::msg)?;
        }
        config.updated_at_ms = now_ms();
        *self.bug_monitor_config.write().await = config.clone();
        self.persist_bug_monitor_config().await?;
        Ok(config)
    }

    pub async fn load_bug_monitor_drafts(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_drafts_path.exists() {
            self.bug_monitor_drafts_path.clone()
        } else if legacy_failure_reporter_path("failure_reporter_drafts.json").exists() {
            legacy_failure_reporter_path("failure_reporter_drafts.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, BugMonitorDraftRecord>>(&raw)
                .unwrap_or_default();
        *self.bug_monitor_drafts.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_drafts(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_drafts_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_drafts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_drafts_path, payload).await?;
        Ok(())
    }

    pub async fn load_bug_monitor_incidents(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_incidents_path.exists() {
            self.bug_monitor_incidents_path.clone()
        } else if legacy_failure_reporter_path("failure_reporter_incidents.json").exists() {
            legacy_failure_reporter_path("failure_reporter_incidents.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, BugMonitorIncidentRecord>,
        >(&raw)
        .unwrap_or_default();
        *self.bug_monitor_incidents.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_incidents(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_incidents_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_incidents.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_incidents_path, payload).await?;
        Ok(())
    }

    pub async fn load_bug_monitor_posts(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_posts_path.exists() {
            self.bug_monitor_posts_path.clone()
        } else if legacy_failure_reporter_path("failure_reporter_posts.json").exists() {
            legacy_failure_reporter_path("failure_reporter_posts.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, BugMonitorPostRecord>>(&raw)
                .unwrap_or_default();
        *self.bug_monitor_posts.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_posts(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_posts_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_posts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_posts_path, payload).await?;
        Ok(())
    }

    pub async fn list_bug_monitor_incidents(&self, limit: usize) -> Vec<BugMonitorIncidentRecord> {
        let mut rows = self
            .bug_monitor_incidents
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_incident(
        &self,
        incident_id: &str,
    ) -> Option<BugMonitorIncidentRecord> {
        self.bug_monitor_incidents
            .read()
            .await
            .get(incident_id)
            .cloned()
    }

    pub async fn put_bug_monitor_incident(
        &self,
        incident: BugMonitorIncidentRecord,
    ) -> anyhow::Result<BugMonitorIncidentRecord> {
        self.bug_monitor_incidents
            .write()
            .await
            .insert(incident.incident_id.clone(), incident.clone());
        self.persist_bug_monitor_incidents().await?;
        Ok(incident)
    }

    pub async fn list_bug_monitor_posts(&self, limit: usize) -> Vec<BugMonitorPostRecord> {
        let mut rows = self
            .bug_monitor_posts
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_post(&self, post_id: &str) -> Option<BugMonitorPostRecord> {
        self.bug_monitor_posts.read().await.get(post_id).cloned()
    }

    pub async fn put_bug_monitor_post(
        &self,
        post: BugMonitorPostRecord,
    ) -> anyhow::Result<BugMonitorPostRecord> {
        self.bug_monitor_posts
            .write()
            .await
            .insert(post.post_id.clone(), post.clone());
        self.persist_bug_monitor_posts().await?;
        Ok(post)
    }

    pub async fn update_bug_monitor_runtime_status(
        &self,
        update: impl FnOnce(&mut BugMonitorRuntimeStatus),
    ) -> BugMonitorRuntimeStatus {
        let mut guard = self.bug_monitor_runtime_status.write().await;
        update(&mut guard);
        guard.clone()
    }

    pub async fn list_bug_monitor_drafts(&self, limit: usize) -> Vec<BugMonitorDraftRecord> {
        let mut rows = self
            .bug_monitor_drafts
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_draft(&self, draft_id: &str) -> Option<BugMonitorDraftRecord> {
        self.bug_monitor_drafts.read().await.get(draft_id).cloned()
    }

    pub async fn put_bug_monitor_draft(
        &self,
        draft: BugMonitorDraftRecord,
    ) -> anyhow::Result<BugMonitorDraftRecord> {
        self.bug_monitor_drafts
            .write()
            .await
            .insert(draft.draft_id.clone(), draft.clone());
        self.persist_bug_monitor_drafts().await?;
        Ok(draft)
    }

    pub async fn submit_bug_monitor_draft(
        &self,
        mut submission: BugMonitorSubmission,
    ) -> anyhow::Result<BugMonitorDraftRecord> {
        fn normalize_optional(value: Option<String>) -> Option<String> {
            value
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        }

        fn compute_fingerprint(parts: &[&str]) -> String {
            use std::hash::{Hash, Hasher};

            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for part in parts {
                part.hash(&mut hasher);
            }
            format!("{:016x}", hasher.finish())
        }

        submission.repo = normalize_optional(submission.repo);
        submission.title = normalize_optional(submission.title);
        submission.detail = normalize_optional(submission.detail);
        submission.source = normalize_optional(submission.source);
        submission.run_id = normalize_optional(submission.run_id);
        submission.session_id = normalize_optional(submission.session_id);
        submission.correlation_id = normalize_optional(submission.correlation_id);
        submission.file_name = normalize_optional(submission.file_name);
        submission.process = normalize_optional(submission.process);
        submission.component = normalize_optional(submission.component);
        submission.event = normalize_optional(submission.event);
        submission.level = normalize_optional(submission.level);
        submission.fingerprint = normalize_optional(submission.fingerprint);
        submission.excerpt = submission
            .excerpt
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .filter(|line| !line.is_empty())
            .take(50)
            .collect();

        let config = self.bug_monitor_config().await;
        let repo = submission
            .repo
            .clone()
            .or(config.repo.clone())
            .ok_or_else(|| anyhow::anyhow!("Bug Monitor repo is not configured"))?;
        if !is_valid_owner_repo_slug(&repo) {
            anyhow::bail!("Bug Monitor repo must be in owner/repo format");
        }

        let title = submission.title.clone().unwrap_or_else(|| {
            if let Some(event) = submission.event.as_ref() {
                format!("Failure detected in {event}")
            } else if let Some(component) = submission.component.as_ref() {
                format!("Failure detected in {component}")
            } else if let Some(process) = submission.process.as_ref() {
                format!("Failure detected in {process}")
            } else if let Some(source) = submission.source.as_ref() {
                format!("Failure report from {source}")
            } else {
                "Failure report".to_string()
            }
        });

        let mut detail_lines = Vec::new();
        if let Some(source) = submission.source.as_ref() {
            detail_lines.push(format!("source: {source}"));
        }
        if let Some(file_name) = submission.file_name.as_ref() {
            detail_lines.push(format!("file: {file_name}"));
        }
        if let Some(level) = submission.level.as_ref() {
            detail_lines.push(format!("level: {level}"));
        }
        if let Some(process) = submission.process.as_ref() {
            detail_lines.push(format!("process: {process}"));
        }
        if let Some(component) = submission.component.as_ref() {
            detail_lines.push(format!("component: {component}"));
        }
        if let Some(event) = submission.event.as_ref() {
            detail_lines.push(format!("event: {event}"));
        }
        if let Some(run_id) = submission.run_id.as_ref() {
            detail_lines.push(format!("run_id: {run_id}"));
        }
        if let Some(session_id) = submission.session_id.as_ref() {
            detail_lines.push(format!("session_id: {session_id}"));
        }
        if let Some(correlation_id) = submission.correlation_id.as_ref() {
            detail_lines.push(format!("correlation_id: {correlation_id}"));
        }
        if let Some(detail) = submission.detail.as_ref() {
            detail_lines.push(String::new());
            detail_lines.push(detail.clone());
        }
        if !submission.excerpt.is_empty() {
            if !detail_lines.is_empty() {
                detail_lines.push(String::new());
            }
            detail_lines.push("excerpt:".to_string());
            detail_lines.extend(submission.excerpt.iter().map(|line| format!("  {line}")));
        }
        let detail = if detail_lines.is_empty() {
            None
        } else {
            Some(detail_lines.join("\n"))
        };

        let fingerprint = submission.fingerprint.clone().unwrap_or_else(|| {
            compute_fingerprint(&[
                repo.as_str(),
                title.as_str(),
                detail.as_deref().unwrap_or(""),
                submission.source.as_deref().unwrap_or(""),
                submission.run_id.as_deref().unwrap_or(""),
                submission.session_id.as_deref().unwrap_or(""),
                submission.correlation_id.as_deref().unwrap_or(""),
            ])
        });

        let mut drafts = self.bug_monitor_drafts.write().await;
        if let Some(existing) = drafts
            .values()
            .find(|row| row.repo == repo && row.fingerprint == fingerprint)
            .cloned()
        {
            return Ok(existing);
        }

        let draft = BugMonitorDraftRecord {
            draft_id: format!("failure-draft-{}", uuid::Uuid::new_v4().simple()),
            fingerprint,
            repo,
            status: if config.require_approval_for_new_issues {
                "approval_required".to_string()
            } else {
                "draft_ready".to_string()
            },
            created_at_ms: now_ms(),
            triage_run_id: None,
            issue_number: None,
            title: Some(title),
            detail,
            github_status: None,
            github_issue_url: None,
            github_comment_url: None,
            github_posted_at_ms: None,
            matched_issue_number: None,
            matched_issue_state: None,
            evidence_digest: None,
            last_post_error: None,
        };
        drafts.insert(draft.draft_id.clone(), draft.clone());
        drop(drafts);
        self.persist_bug_monitor_drafts().await?;
        Ok(draft)
    }

    pub async fn update_bug_monitor_draft_status(
        &self,
        draft_id: &str,
        next_status: &str,
        reason: Option<&str>,
    ) -> anyhow::Result<BugMonitorDraftRecord> {
        let normalized_status = next_status.trim().to_ascii_lowercase();
        if normalized_status != "draft_ready" && normalized_status != "denied" {
            anyhow::bail!("unsupported Bug Monitor draft status");
        }

        let mut drafts = self.bug_monitor_drafts.write().await;
        let Some(draft) = drafts.get_mut(draft_id) else {
            anyhow::bail!("Bug Monitor draft not found");
        };
        if !draft.status.eq_ignore_ascii_case("approval_required") {
            anyhow::bail!("Bug Monitor draft is not waiting for approval");
        }
        draft.status = normalized_status.clone();
        if let Some(reason) = reason
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            let next_detail = if let Some(detail) = draft.detail.as_ref() {
                format!("{detail}\n\noperator_note: {reason}")
            } else {
                format!("operator_note: {reason}")
            };
            draft.detail = Some(next_detail);
        }
        let updated = draft.clone();
        drop(drafts);
        self.persist_bug_monitor_drafts().await?;

        let event_name = if normalized_status == "draft_ready" {
            "bug_monitor.draft.approved"
        } else {
            "bug_monitor.draft.denied"
        };
        self.event_bus.publish(EngineEvent::new(
            event_name,
            serde_json::json!({
                "draft_id": updated.draft_id,
                "repo": updated.repo,
                "status": updated.status,
                "reason": reason,
            }),
        ));
        Ok(updated)
    }

    pub async fn bug_monitor_status(&self) -> BugMonitorStatus {
        let required_capabilities = vec![
            "github.list_issues".to_string(),
            "github.get_issue".to_string(),
            "github.create_issue".to_string(),
            "github.comment_on_issue".to_string(),
        ];
        let config = self.bug_monitor_config().await;
        let drafts = self.bug_monitor_drafts.read().await;
        let incidents = self.bug_monitor_incidents.read().await;
        let posts = self.bug_monitor_posts.read().await;
        let total_incidents = incidents.len();
        let pending_incidents = incidents
            .values()
            .filter(|row| {
                matches!(
                    row.status.as_str(),
                    "queued"
                        | "draft_created"
                        | "triage_queued"
                        | "analysis_queued"
                        | "triage_pending"
                        | "issue_draft_pending"
                )
            })
            .count();
        let pending_drafts = drafts
            .values()
            .filter(|row| row.status.eq_ignore_ascii_case("approval_required"))
            .count();
        let pending_posts = posts
            .values()
            .filter(|row| matches!(row.status.as_str(), "queued" | "failed"))
            .count();
        let last_activity_at_ms = drafts
            .values()
            .map(|row| row.created_at_ms)
            .chain(posts.values().map(|row| row.updated_at_ms))
            .max();
        drop(drafts);
        drop(incidents);
        drop(posts);
        let mut runtime = self.bug_monitor_runtime_status.read().await.clone();
        runtime.paused = config.paused;
        runtime.total_incidents = total_incidents;
        runtime.pending_incidents = pending_incidents;
        runtime.pending_posts = pending_posts;

        let mut status = BugMonitorStatus {
            config: config.clone(),
            runtime,
            pending_drafts,
            pending_posts,
            last_activity_at_ms,
            ..BugMonitorStatus::default()
        };
        let repo_valid = config
            .repo
            .as_ref()
            .map(|repo| is_valid_owner_repo_slug(repo))
            .unwrap_or(false);
        let servers = self.mcp.list().await;
        let selected_server = config
            .mcp_server
            .as_ref()
            .and_then(|name| servers.get(name))
            .cloned();
        let provider_catalog = self.providers.list().await;
        let selected_model = config
            .model_policy
            .as_ref()
            .and_then(|policy| policy.get("default_model"))
            .and_then(parse_model_spec);
        let selected_model_ready = selected_model
            .as_ref()
            .map(|spec| provider_catalog_has_model(&provider_catalog, spec))
            .unwrap_or(false);
        let selected_server_tools = if let Some(server_name) = config.mcp_server.as_ref() {
            self.mcp.server_tools(server_name).await
        } else {
            Vec::new()
        };
        let discovered_tools = self
            .capability_resolver
            .discover_from_runtime(selected_server_tools, Vec::new())
            .await;
        status.discovered_mcp_tools = discovered_tools
            .iter()
            .map(|row| row.tool_name.clone())
            .collect();
        let discovered_providers = discovered_tools
            .iter()
            .map(|row| row.provider.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let provider_preference = match config.provider_preference {
            BugMonitorProviderPreference::OfficialGithub => {
                vec![
                    "mcp".to_string(),
                    "composio".to_string(),
                    "arcade".to_string(),
                ]
            }
            BugMonitorProviderPreference::Composio => {
                vec![
                    "composio".to_string(),
                    "mcp".to_string(),
                    "arcade".to_string(),
                ]
            }
            BugMonitorProviderPreference::Arcade => {
                vec![
                    "arcade".to_string(),
                    "mcp".to_string(),
                    "composio".to_string(),
                ]
            }
            BugMonitorProviderPreference::Auto => {
                vec![
                    "mcp".to_string(),
                    "composio".to_string(),
                    "arcade".to_string(),
                ]
            }
        };
        let capability_resolution = self
            .capability_resolver
            .resolve(
                crate::capability_resolver::CapabilityResolveInput {
                    workflow_id: Some("bug_monitor".to_string()),
                    required_capabilities: required_capabilities.clone(),
                    optional_capabilities: Vec::new(),
                    provider_preference,
                    available_tools: discovered_tools,
                },
                Vec::new(),
            )
            .await
            .ok();
        let bindings_file = self.capability_resolver.list_bindings().await.ok();
        if let Some(bindings) = bindings_file.as_ref() {
            status.binding_source_version = bindings.builtin_version.clone();
            status.bindings_last_merged_at_ms = bindings.last_merged_at_ms;
            status.selected_server_binding_candidates = bindings
                .bindings
                .iter()
                .filter(|binding| required_capabilities.contains(&binding.capability_id))
                .filter(|binding| {
                    discovered_providers.is_empty()
                        || discovered_providers.contains(&binding.provider.to_ascii_lowercase())
                })
                .map(|binding| {
                    let binding_key = format!(
                        "{}::{}",
                        binding.capability_id,
                        binding.tool_name.to_ascii_lowercase()
                    );
                    let matched = capability_resolution
                        .as_ref()
                        .map(|resolution| {
                            resolution.resolved.iter().any(|row| {
                                row.capability_id == binding.capability_id
                                    && format!(
                                        "{}::{}",
                                        row.capability_id,
                                        row.tool_name.to_ascii_lowercase()
                                    ) == binding_key
                            })
                        })
                        .unwrap_or(false);
                    BugMonitorBindingCandidate {
                        capability_id: binding.capability_id.clone(),
                        binding_tool_name: binding.tool_name.clone(),
                        aliases: binding.tool_name_aliases.clone(),
                        matched,
                    }
                })
                .collect();
            status.selected_server_binding_candidates.sort_by(|a, b| {
                a.capability_id
                    .cmp(&b.capability_id)
                    .then_with(|| a.binding_tool_name.cmp(&b.binding_tool_name))
            });
        }
        let capability_ready = |capability_id: &str| -> bool {
            capability_resolution
                .as_ref()
                .map(|resolved| {
                    resolved
                        .resolved
                        .iter()
                        .any(|row| row.capability_id == capability_id)
                })
                .unwrap_or(false)
        };
        if let Some(resolution) = capability_resolution.as_ref() {
            status.missing_required_capabilities = resolution.missing_required.clone();
            status.resolved_capabilities = resolution
                .resolved
                .iter()
                .map(|row| BugMonitorCapabilityMatch {
                    capability_id: row.capability_id.clone(),
                    provider: row.provider.clone(),
                    tool_name: row.tool_name.clone(),
                    binding_index: row.binding_index,
                })
                .collect();
        } else {
            status.missing_required_capabilities = required_capabilities.clone();
        }
        status.required_capabilities = BugMonitorCapabilityReadiness {
            github_list_issues: capability_ready("github.list_issues"),
            github_get_issue: capability_ready("github.get_issue"),
            github_create_issue: capability_ready("github.create_issue"),
            github_comment_on_issue: capability_ready("github.comment_on_issue"),
        };
        status.selected_model = selected_model;
        status.readiness = BugMonitorReadiness {
            config_valid: repo_valid
                && selected_server.is_some()
                && status.required_capabilities.github_list_issues
                && status.required_capabilities.github_get_issue
                && status.required_capabilities.github_create_issue
                && status.required_capabilities.github_comment_on_issue
                && selected_model_ready,
            repo_valid,
            mcp_server_present: selected_server.is_some(),
            mcp_connected: selected_server
                .as_ref()
                .map(|row| row.connected)
                .unwrap_or(false),
            github_read_ready: status.required_capabilities.github_list_issues
                && status.required_capabilities.github_get_issue,
            github_write_ready: status.required_capabilities.github_create_issue
                && status.required_capabilities.github_comment_on_issue,
            selected_model_ready,
            ingest_ready: config.enabled && !config.paused && repo_valid,
            publish_ready: config.enabled
                && !config.paused
                && repo_valid
                && selected_server
                    .as_ref()
                    .map(|row| row.connected)
                    .unwrap_or(false)
                && status.required_capabilities.github_list_issues
                && status.required_capabilities.github_get_issue
                && status.required_capabilities.github_create_issue
                && status.required_capabilities.github_comment_on_issue
                && selected_model_ready,
            runtime_ready: config.enabled
                && !config.paused
                && repo_valid
                && selected_server
                    .as_ref()
                    .map(|row| row.connected)
                    .unwrap_or(false)
                && status.required_capabilities.github_list_issues
                && status.required_capabilities.github_get_issue
                && status.required_capabilities.github_create_issue
                && status.required_capabilities.github_comment_on_issue
                && selected_model_ready,
        };
        if config.enabled {
            if config.paused {
                status.last_error = Some("Bug monitor monitoring is paused.".to_string());
            } else if !repo_valid {
                status.last_error = Some("Target repo is missing or invalid.".to_string());
            } else if selected_server.is_none() {
                status.last_error = Some("Selected MCP server is missing.".to_string());
            } else if !status.readiness.mcp_connected {
                status.last_error = Some("Selected MCP server is disconnected.".to_string());
            } else if !selected_model_ready {
                status.last_error = Some(
                    "Selected provider/model is unavailable. Bug monitor is fail-closed."
                        .to_string(),
                );
            } else if !status.readiness.github_read_ready || !status.readiness.github_write_ready {
                let missing = if status.missing_required_capabilities.is_empty() {
                    "unknown".to_string()
                } else {
                    status.missing_required_capabilities.join(", ")
                };
                status.last_error = Some(format!(
                    "Selected MCP server is missing required GitHub capabilities: {missing}"
                ));
            }
        }
        status.runtime.monitoring_active = status.readiness.ingest_ready;
        status
    }

    pub async fn load_workflow_runs(&self) -> anyhow::Result<()> {
        if !self.workflow_runs_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.workflow_runs_path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, WorkflowRunRecord>>(&raw)
                .unwrap_or_default();
        *self.workflow_runs.write().await = parsed;
        Ok(())
    }

    pub async fn persist_workflow_runs(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.workflow_runs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.workflow_runs.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.workflow_runs_path, payload).await?;
        Ok(())
    }

    pub async fn load_workflow_hook_overrides(&self) -> anyhow::Result<()> {
        if !self.workflow_hook_overrides_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.workflow_hook_overrides_path).await?;
        let parsed = serde_json::from_str::<std::collections::HashMap<String, bool>>(&raw)
            .unwrap_or_default();
        *self.workflow_hook_overrides.write().await = parsed;
        Ok(())
    }

    pub async fn persist_workflow_hook_overrides(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.workflow_hook_overrides_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.workflow_hook_overrides.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.workflow_hook_overrides_path, payload).await?;
        Ok(())
    }

    pub async fn reload_workflows(&self) -> anyhow::Result<Vec<WorkflowValidationMessage>> {
        let mut sources = Vec::new();
        sources.push(WorkflowLoadSource {
            root: resolve_builtin_workflows_dir(),
            kind: WorkflowSourceKind::BuiltIn,
            pack_id: None,
        });

        let workspace_root = self.workspace_index.snapshot().await.root;
        sources.push(WorkflowLoadSource {
            root: PathBuf::from(workspace_root).join(".tandem"),
            kind: WorkflowSourceKind::Workspace,
            pack_id: None,
        });

        if let Ok(packs) = self.pack_manager.list().await {
            for pack in packs {
                sources.push(WorkflowLoadSource {
                    root: PathBuf::from(pack.install_path),
                    kind: WorkflowSourceKind::Pack,
                    pack_id: Some(pack.pack_id),
                });
            }
        }

        let mut registry = load_workflow_registry(&sources)?;
        let overrides = self.workflow_hook_overrides.read().await.clone();
        for hook in &mut registry.hooks {
            if let Some(enabled) = overrides.get(&hook.binding_id) {
                hook.enabled = *enabled;
            }
        }
        for workflow in registry.workflows.values_mut() {
            workflow.hooks = registry
                .hooks
                .iter()
                .filter(|hook| hook.workflow_id == workflow.workflow_id)
                .cloned()
                .collect();
        }
        let messages = validate_workflow_registry(&registry);
        *self.workflows.write().await = registry;
        Ok(messages)
    }

    pub async fn workflow_registry(&self) -> WorkflowRegistry {
        self.workflows.read().await.clone()
    }

    pub async fn list_workflows(&self) -> Vec<WorkflowSpec> {
        let mut rows = self
            .workflows
            .read()
            .await
            .workflows
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.workflow_id.cmp(&b.workflow_id));
        rows
    }

    pub async fn get_workflow(&self, workflow_id: &str) -> Option<WorkflowSpec> {
        self.workflows
            .read()
            .await
            .workflows
            .get(workflow_id)
            .cloned()
    }

    pub async fn list_workflow_hooks(&self, workflow_id: Option<&str>) -> Vec<WorkflowHookBinding> {
        let mut rows = self
            .workflows
            .read()
            .await
            .hooks
            .iter()
            .filter(|hook| workflow_id.map(|id| hook.workflow_id == id).unwrap_or(true))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.binding_id.cmp(&b.binding_id));
        rows
    }

    pub async fn set_workflow_hook_enabled(
        &self,
        binding_id: &str,
        enabled: bool,
    ) -> anyhow::Result<Option<WorkflowHookBinding>> {
        self.workflow_hook_overrides
            .write()
            .await
            .insert(binding_id.to_string(), enabled);
        self.persist_workflow_hook_overrides().await?;
        let _ = self.reload_workflows().await?;
        Ok(self
            .workflows
            .read()
            .await
            .hooks
            .iter()
            .find(|hook| hook.binding_id == binding_id)
            .cloned())
    }

    pub async fn put_workflow_run(&self, run: WorkflowRunRecord) -> anyhow::Result<()> {
        self.workflow_runs
            .write()
            .await
            .insert(run.run_id.clone(), run);
        self.persist_workflow_runs().await
    }

    pub async fn update_workflow_run(
        &self,
        run_id: &str,
        update: impl FnOnce(&mut WorkflowRunRecord),
    ) -> Option<WorkflowRunRecord> {
        let mut guard = self.workflow_runs.write().await;
        let row = guard.get_mut(run_id)?;
        update(row);
        row.updated_at_ms = now_ms();
        if matches!(
            row.status,
            WorkflowRunStatus::Completed | WorkflowRunStatus::Failed
        ) {
            row.finished_at_ms.get_or_insert_with(now_ms);
        }
        let out = row.clone();
        drop(guard);
        let _ = self.persist_workflow_runs().await;
        Some(out)
    }

    pub async fn list_workflow_runs(
        &self,
        workflow_id: Option<&str>,
        limit: usize,
    ) -> Vec<WorkflowRunRecord> {
        let mut rows = self
            .workflow_runs
            .read()
            .await
            .values()
            .filter(|row| workflow_id.map(|id| row.workflow_id == id).unwrap_or(true))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn get_workflow_run(&self, run_id: &str) -> Option<WorkflowRunRecord> {
        self.workflow_runs.read().await.get(run_id).cloned()
    }

    pub async fn put_automation_v2(
        &self,
        mut automation: AutomationV2Spec,
    ) -> anyhow::Result<AutomationV2Spec> {
        if automation.automation_id.trim().is_empty() {
            anyhow::bail!("automation_id is required");
        }
        for agent in &mut automation.agents {
            if agent.display_name.trim().is_empty() {
                agent.display_name = auto_generated_agent_name(&agent.agent_id);
            }
            agent.tool_policy.allowlist =
                normalize_allowed_tools(agent.tool_policy.allowlist.clone());
            agent.tool_policy.denylist =
                normalize_allowed_tools(agent.tool_policy.denylist.clone());
            agent.mcp_policy.allowed_servers =
                normalize_non_empty_list(agent.mcp_policy.allowed_servers.clone());
            agent.mcp_policy.allowed_tools = agent
                .mcp_policy
                .allowed_tools
                .take()
                .map(normalize_allowed_tools);
        }
        let now = now_ms();
        if automation.created_at_ms == 0 {
            automation.created_at_ms = now;
        }
        automation.updated_at_ms = now;
        if automation.next_fire_at_ms.is_none() {
            automation.next_fire_at_ms =
                automation_schedule_next_fire_at_ms(&automation.schedule, now);
        }
        self.automations_v2
            .write()
            .await
            .insert(automation.automation_id.clone(), automation.clone());
        self.persist_automations_v2().await?;
        self.verify_automation_v2_persisted(&automation.automation_id, true)
            .await?;
        Ok(automation)
    }

    pub async fn get_automation_v2(&self, automation_id: &str) -> Option<AutomationV2Spec> {
        self.automations_v2.read().await.get(automation_id).cloned()
    }

    pub async fn put_workflow_plan(&self, plan: WorkflowPlan) {
        self.workflow_plans
            .write()
            .await
            .insert(plan.plan_id.clone(), plan);
    }

    pub async fn get_workflow_plan(&self, plan_id: &str) -> Option<WorkflowPlan> {
        self.workflow_plans.read().await.get(plan_id).cloned()
    }

    pub async fn put_workflow_plan_draft(&self, draft: WorkflowPlanDraftRecord) {
        self.workflow_plan_drafts
            .write()
            .await
            .insert(draft.current_plan.plan_id.clone(), draft.clone());
        self.put_workflow_plan(draft.current_plan).await;
    }

    pub async fn get_workflow_plan_draft(&self, plan_id: &str) -> Option<WorkflowPlanDraftRecord> {
        self.workflow_plan_drafts.read().await.get(plan_id).cloned()
    }

    pub async fn list_automations_v2(&self) -> Vec<AutomationV2Spec> {
        let mut rows = self
            .automations_v2
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.automation_id.cmp(&b.automation_id));
        rows
    }

    pub async fn delete_automation_v2(
        &self,
        automation_id: &str,
    ) -> anyhow::Result<Option<AutomationV2Spec>> {
        let removed = self.automations_v2.write().await.remove(automation_id);
        self.persist_automations_v2().await?;
        self.verify_automation_v2_persisted(automation_id, false)
            .await?;
        Ok(removed)
    }

    pub async fn create_automation_v2_run(
        &self,
        automation: &AutomationV2Spec,
        trigger_type: &str,
    ) -> anyhow::Result<AutomationV2RunRecord> {
        let now = now_ms();
        let pending_nodes = automation
            .flow
            .nodes
            .iter()
            .map(|n| n.node_id.clone())
            .collect::<Vec<_>>();
        let run = AutomationV2RunRecord {
            run_id: format!("automation-v2-run-{}", uuid::Uuid::new_v4()),
            automation_id: automation.automation_id.clone(),
            trigger_type: trigger_type.to_string(),
            status: AutomationRunStatus::Queued,
            created_at_ms: now,
            updated_at_ms: now,
            started_at_ms: None,
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes,
                node_outputs: std::collections::HashMap::new(),
                node_attempts: std::collections::HashMap::new(),
            },
            automation_snapshot: Some(automation.clone()),
            pause_reason: None,
            resume_reason: None,
            detail: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        };
        self.automation_v2_runs
            .write()
            .await
            .insert(run.run_id.clone(), run.clone());
        self.persist_automation_v2_runs().await?;
        Ok(run)
    }

    pub async fn get_automation_v2_run(&self, run_id: &str) -> Option<AutomationV2RunRecord> {
        self.automation_v2_runs.read().await.get(run_id).cloned()
    }

    pub async fn list_automation_v2_runs(
        &self,
        automation_id: Option<&str>,
        limit: usize,
    ) -> Vec<AutomationV2RunRecord> {
        let mut rows = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|row| {
                if let Some(id) = automation_id {
                    row.automation_id == id
                } else {
                    true
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn claim_next_queued_automation_v2_run(&self) -> Option<AutomationV2RunRecord> {
        let mut guard = self.automation_v2_runs.write().await;
        let run_id = guard
            .values()
            .filter(|row| row.status == AutomationRunStatus::Queued)
            .min_by(|a, b| a.created_at_ms.cmp(&b.created_at_ms))
            .map(|row| row.run_id.clone())?;
        let now = now_ms();
        let run = guard.get_mut(&run_id)?;
        run.status = AutomationRunStatus::Running;
        run.updated_at_ms = now;
        run.started_at_ms.get_or_insert(now);
        let claimed = run.clone();
        drop(guard);
        let _ = self.persist_automation_v2_runs().await;
        Some(claimed)
    }

    pub async fn update_automation_v2_run(
        &self,
        run_id: &str,
        update: impl FnOnce(&mut AutomationV2RunRecord),
    ) -> Option<AutomationV2RunRecord> {
        let mut guard = self.automation_v2_runs.write().await;
        let run = guard.get_mut(run_id)?;
        update(run);
        run.updated_at_ms = now_ms();
        if matches!(
            run.status,
            AutomationRunStatus::Completed
                | AutomationRunStatus::Failed
                | AutomationRunStatus::Cancelled
        ) {
            run.finished_at_ms.get_or_insert_with(now_ms);
        }
        let out = run.clone();
        drop(guard);
        let _ = self.persist_automation_v2_runs().await;
        Some(out)
    }

    pub async fn add_automation_v2_session(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        let updated = self
            .update_automation_v2_run(run_id, |row| {
                if !row.active_session_ids.iter().any(|id| id == session_id) {
                    row.active_session_ids.push(session_id.to_string());
                }
            })
            .await;
        self.automation_v2_session_runs
            .write()
            .await
            .insert(session_id.to_string(), run_id.to_string());
        updated
    }

    pub async fn clear_automation_v2_session(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.automation_v2_session_runs
            .write()
            .await
            .remove(session_id);
        self.update_automation_v2_run(run_id, |row| {
            row.active_session_ids.retain(|id| id != session_id);
        })
        .await
    }

    pub async fn apply_provider_usage_to_runs(
        &self,
        session_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
    ) {
        if let Some(policy) = self.routine_session_policy(session_id).await {
            let rate = self.token_cost_per_1k_usd.max(0.0);
            let delta_cost = (total_tokens as f64 / 1000.0) * rate;
            let mut guard = self.routine_runs.write().await;
            if let Some(run) = guard.get_mut(&policy.run_id) {
                run.prompt_tokens = run.prompt_tokens.saturating_add(prompt_tokens);
                run.completion_tokens = run.completion_tokens.saturating_add(completion_tokens);
                run.total_tokens = run.total_tokens.saturating_add(total_tokens);
                run.estimated_cost_usd += delta_cost;
                run.updated_at_ms = now_ms();
            }
            drop(guard);
            let _ = self.persist_routine_runs().await;
        }

        let maybe_v2_run_id = self
            .automation_v2_session_runs
            .read()
            .await
            .get(session_id)
            .cloned();
        if let Some(run_id) = maybe_v2_run_id {
            let rate = self.token_cost_per_1k_usd.max(0.0);
            let delta_cost = (total_tokens as f64 / 1000.0) * rate;
            let mut guard = self.automation_v2_runs.write().await;
            if let Some(run) = guard.get_mut(&run_id) {
                run.prompt_tokens = run.prompt_tokens.saturating_add(prompt_tokens);
                run.completion_tokens = run.completion_tokens.saturating_add(completion_tokens);
                run.total_tokens = run.total_tokens.saturating_add(total_tokens);
                run.estimated_cost_usd += delta_cost;
                run.updated_at_ms = now_ms();
            }
            drop(guard);
            let _ = self.persist_automation_v2_runs().await;
        }
    }

    pub async fn evaluate_automation_v2_misfires(&self, now_ms: u64) -> Vec<String> {
        let mut fired = Vec::new();
        let mut guard = self.automations_v2.write().await;
        for automation in guard.values_mut() {
            if automation.status != AutomationV2Status::Active {
                continue;
            }
            let Some(next_fire_at_ms) = automation.next_fire_at_ms else {
                automation.next_fire_at_ms =
                    automation_schedule_next_fire_at_ms(&automation.schedule, now_ms);
                continue;
            };
            if now_ms < next_fire_at_ms {
                continue;
            }
            let run_count =
                automation_schedule_due_count(&automation.schedule, now_ms, next_fire_at_ms);
            let next = automation_schedule_next_fire_at_ms(&automation.schedule, now_ms);
            automation.next_fire_at_ms = next;
            automation.last_fired_at_ms = Some(now_ms);
            for _ in 0..run_count {
                fired.push(automation.automation_id.clone());
            }
        }
        drop(guard);
        let _ = self.persist_automations_v2().await;
        fired
    }
}

async fn build_channels_config(
    state: &AppState,
    channels: &ChannelsConfigFile,
) -> Option<ChannelsConfig> {
    if channels.telegram.is_none() && channels.discord.is_none() && channels.slack.is_none() {
        return None;
    }
    Some(ChannelsConfig {
        telegram: channels.telegram.clone().map(|cfg| TelegramConfig {
            bot_token: cfg.bot_token,
            allowed_users: cfg.allowed_users,
            mention_only: cfg.mention_only,
            style_profile: cfg.style_profile,
        }),
        discord: channels.discord.clone().map(|cfg| DiscordConfig {
            bot_token: cfg.bot_token,
            guild_id: cfg.guild_id,
            allowed_users: cfg.allowed_users,
            mention_only: cfg.mention_only,
        }),
        slack: channels.slack.clone().map(|cfg| SlackConfig {
            bot_token: cfg.bot_token,
            channel_id: cfg.channel_id,
            allowed_users: cfg.allowed_users,
            mention_only: cfg.mention_only,
        }),
        server_base_url: state.server_base_url(),
        api_token: state.api_token().await.unwrap_or_default(),
        tool_policy: channels.tool_policy.clone(),
    })
}

fn normalize_web_ui_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/admin".to_string();
    }
    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    with_leading.trim_end_matches('/').to_string()
}

fn default_web_ui_prefix() -> String {
    "/admin".to_string()
}

fn default_allow_all() -> Vec<String> {
    vec!["*".to_string()]
}

fn default_discord_mention_only() -> bool {
    true
}

fn normalize_allowed_tools(raw: Vec<String>) -> Vec<String> {
    normalize_non_empty_list(raw)
}

fn normalize_non_empty_list(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in raw {
        let normalized = item.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    out
}

fn resolve_run_stale_ms() -> u64 {
    std::env::var("TANDEM_RUN_STALE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(120_000)
        .clamp(30_000, 600_000)
}

fn resolve_token_cost_per_1k_usd() -> f64 {
    std::env::var("TANDEM_TOKEN_COST_PER_1K_USD")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
        .max(0.0)
}

fn default_true() -> bool {
    true
}

fn parse_bool_env(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

fn resolve_bug_monitor_env_config() -> BugMonitorConfig {
    fn env_value(new_name: &str, legacy_name: &str) -> Option<String> {
        std::env::var(new_name)
            .ok()
            .or_else(|| std::env::var(legacy_name).ok())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    fn env_bool(new_name: &str, legacy_name: &str, default: bool) -> bool {
        env_value(new_name, legacy_name)
            .map(|value| parse_bool_like(&value, default))
            .unwrap_or(default)
    }

    fn parse_bool_like(value: &str, default: bool) -> bool {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        }
    }

    let provider_preference = match env_value(
        "TANDEM_BUG_MONITOR_PROVIDER_PREFERENCE",
        "TANDEM_FAILURE_REPORTER_PROVIDER_PREFERENCE",
    )
    .unwrap_or_default()
    .trim()
    .to_ascii_lowercase()
    .as_str()
    {
        "official_github" | "official-github" | "github" => {
            BugMonitorProviderPreference::OfficialGithub
        }
        "composio" => BugMonitorProviderPreference::Composio,
        "arcade" => BugMonitorProviderPreference::Arcade,
        _ => BugMonitorProviderPreference::Auto,
    };
    let provider_id = env_value(
        "TANDEM_BUG_MONITOR_PROVIDER_ID",
        "TANDEM_FAILURE_REPORTER_PROVIDER_ID",
    );
    let model_id = env_value(
        "TANDEM_BUG_MONITOR_MODEL_ID",
        "TANDEM_FAILURE_REPORTER_MODEL_ID",
    );
    let model_policy = match (provider_id, model_id) {
        (Some(provider_id), Some(model_id)) => Some(json!({
            "default_model": {
                "provider_id": provider_id,
                "model_id": model_id,
            }
        })),
        _ => None,
    };
    BugMonitorConfig {
        enabled: env_bool(
            "TANDEM_BUG_MONITOR_ENABLED",
            "TANDEM_FAILURE_REPORTER_ENABLED",
            false,
        ),
        paused: env_bool(
            "TANDEM_BUG_MONITOR_PAUSED",
            "TANDEM_FAILURE_REPORTER_PAUSED",
            false,
        ),
        workspace_root: env_value(
            "TANDEM_BUG_MONITOR_WORKSPACE_ROOT",
            "TANDEM_FAILURE_REPORTER_WORKSPACE_ROOT",
        ),
        repo: env_value("TANDEM_BUG_MONITOR_REPO", "TANDEM_FAILURE_REPORTER_REPO"),
        mcp_server: env_value(
            "TANDEM_BUG_MONITOR_MCP_SERVER",
            "TANDEM_FAILURE_REPORTER_MCP_SERVER",
        ),
        provider_preference,
        model_policy,
        auto_create_new_issues: env_bool(
            "TANDEM_BUG_MONITOR_AUTO_CREATE_NEW_ISSUES",
            "TANDEM_FAILURE_REPORTER_AUTO_CREATE_NEW_ISSUES",
            true,
        ),
        require_approval_for_new_issues: env_bool(
            "TANDEM_BUG_MONITOR_REQUIRE_APPROVAL_FOR_NEW_ISSUES",
            "TANDEM_FAILURE_REPORTER_REQUIRE_APPROVAL_FOR_NEW_ISSUES",
            false,
        ),
        auto_comment_on_matched_open_issues: env_bool(
            "TANDEM_BUG_MONITOR_AUTO_COMMENT_ON_MATCHED_OPEN_ISSUES",
            "TANDEM_FAILURE_REPORTER_AUTO_COMMENT_ON_MATCHED_OPEN_ISSUES",
            true,
        ),
        label_mode: BugMonitorLabelMode::ReporterOnly,
        updated_at_ms: 0,
    }
}

fn is_valid_owner_repo_slug(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.ends_with('/') {
        return false;
    }
    let mut parts = trimmed.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };
    parts.next().is_none() && !owner.trim().is_empty() && !repo.trim().is_empty()
}

fn resolve_shared_resources_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("shared_resources.json");
        }
    }
    default_state_dir().join("shared_resources.json")
}

fn resolve_routines_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("routines.json");
        }
    }
    default_state_dir().join("routines.json")
}

fn resolve_routine_history_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STORAGE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("routine_history.json");
        }
    }
    default_state_dir().join("routine_history.json")
}

fn resolve_routine_runs_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("routine_runs.json");
        }
    }
    default_state_dir().join("routine_runs.json")
}

fn resolve_automations_v2_path() -> PathBuf {
    resolve_canonical_data_file_path("automations_v2.json")
}

fn legacy_automations_v2_path() -> Option<PathBuf> {
    resolve_legacy_root_file_path("automations_v2.json")
        .filter(|path| path != &resolve_automations_v2_path())
}

fn candidate_automations_v2_paths(active_path: &PathBuf) -> Vec<PathBuf> {
    let mut candidates = vec![active_path.clone()];
    if let Some(legacy_path) = legacy_automations_v2_path() {
        if !candidates.contains(&legacy_path) {
            candidates.push(legacy_path);
        }
    }
    let default_path = default_state_dir().join("automations_v2.json");
    if !candidates.contains(&default_path) {
        candidates.push(default_path);
    }
    candidates
}

fn resolve_automation_v2_runs_path() -> PathBuf {
    resolve_canonical_data_file_path("automation_v2_runs.json")
}

fn legacy_automation_v2_runs_path() -> Option<PathBuf> {
    resolve_legacy_root_file_path("automation_v2_runs.json")
        .filter(|path| path != &resolve_automation_v2_runs_path())
}

fn candidate_automation_v2_runs_paths(active_path: &PathBuf) -> Vec<PathBuf> {
    let mut candidates = vec![active_path.clone()];
    if let Some(legacy_path) = legacy_automation_v2_runs_path() {
        if !candidates.contains(&legacy_path) {
            candidates.push(legacy_path);
        }
    }
    let default_path = default_state_dir().join("automation_v2_runs.json");
    if !candidates.contains(&default_path) {
        candidates.push(default_path);
    }
    candidates
}

fn parse_automation_v2_file(raw: &str) -> std::collections::HashMap<String, AutomationV2Spec> {
    serde_json::from_str::<std::collections::HashMap<String, AutomationV2Spec>>(raw)
        .unwrap_or_default()
}

fn parse_automation_v2_runs_file(
    raw: &str,
) -> std::collections::HashMap<String, AutomationV2RunRecord> {
    serde_json::from_str::<std::collections::HashMap<String, AutomationV2RunRecord>>(raw)
        .unwrap_or_default()
}

fn resolve_canonical_data_file_path(file_name: &str) -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            let base = PathBuf::from(trimmed);
            return if path_is_data_dir(&base) {
                base.join(file_name)
            } else {
                base.join("data").join(file_name)
            };
        }
    }
    default_state_dir().join(file_name)
}

fn resolve_legacy_root_file_path(file_name: &str) -> Option<PathBuf> {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            let base = PathBuf::from(trimmed);
            if !path_is_data_dir(&base) {
                return Some(base.join(file_name));
            }
        }
    }
    resolve_shared_paths()
        .ok()
        .map(|paths| paths.canonical_root.join(file_name))
}

fn path_is_data_dir(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("data"))
        .unwrap_or(false)
}

fn resolve_workflow_runs_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("workflow_runs.json");
        }
    }
    default_state_dir().join("workflow_runs.json")
}

fn resolve_bug_monitor_config_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("bug_monitor_config.json");
        }
    }
    default_state_dir().join("bug_monitor_config.json")
}

fn resolve_bug_monitor_drafts_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("bug_monitor_drafts.json");
        }
    }
    default_state_dir().join("bug_monitor_drafts.json")
}

fn resolve_bug_monitor_incidents_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("bug_monitor_incidents.json");
        }
    }
    default_state_dir().join("bug_monitor_incidents.json")
}

fn resolve_bug_monitor_posts_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("bug_monitor_posts.json");
        }
    }
    default_state_dir().join("bug_monitor_posts.json")
}

fn legacy_failure_reporter_path(file_name: &str) -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join(file_name);
        }
    }
    default_state_dir().join(file_name)
}

fn resolve_workflow_hook_overrides_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("workflow_hook_overrides.json");
        }
    }
    default_state_dir().join("workflow_hook_overrides.json")
}

fn resolve_builtin_workflows_dir() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_BUILTIN_WORKFLOW_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    default_state_dir().join("builtin_workflows")
}

fn resolve_agent_team_audit_path() -> PathBuf {
    if let Ok(base) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = base.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed)
                .join("agent-team")
                .join("audit.log.jsonl");
        }
    }
    default_state_dir()
        .join("agent-team")
        .join("audit.log.jsonl")
}

fn default_state_dir() -> PathBuf {
    if let Ok(paths) = resolve_shared_paths() {
        return paths.engine_state_dir;
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("tandem").join("data");
    }
    dirs::home_dir()
        .map(|home| home.join(".tandem").join("data"))
        .unwrap_or_else(|| PathBuf::from(".tandem"))
}

fn sibling_backup_path(path: &PathBuf) -> PathBuf {
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    let backup_name = format!("{base}.bak");
    path.with_file_name(backup_name)
}

fn sibling_tmp_path(path: &PathBuf) -> PathBuf {
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    let tmp_name = format!("{base}.tmp");
    path.with_file_name(tmp_name)
}

fn routine_interval_ms(schedule: &RoutineSchedule) -> Option<u64> {
    match schedule {
        RoutineSchedule::IntervalSeconds { seconds } => Some(seconds.saturating_mul(1000)),
        RoutineSchedule::Cron { .. } => None,
    }
}

fn parse_timezone(timezone: &str) -> Option<Tz> {
    timezone.trim().parse::<Tz>().ok()
}

fn next_cron_fire_at_ms(expression: &str, timezone: &str, from_ms: u64) -> Option<u64> {
    let tz = parse_timezone(timezone)?;
    let schedule = Schedule::from_str(expression).ok()?;
    let from_dt = Utc.timestamp_millis_opt(from_ms as i64).single()?;
    let local_from = from_dt.with_timezone(&tz);
    let next = schedule.after(&local_from).next()?;
    Some(next.with_timezone(&Utc).timestamp_millis().max(0) as u64)
}

fn compute_next_schedule_fire_at_ms(
    schedule: &RoutineSchedule,
    timezone: &str,
    from_ms: u64,
) -> Option<u64> {
    let _ = parse_timezone(timezone)?;
    match schedule {
        RoutineSchedule::IntervalSeconds { seconds } => {
            Some(from_ms.saturating_add(seconds.saturating_mul(1000)))
        }
        RoutineSchedule::Cron { expression } => next_cron_fire_at_ms(expression, timezone, from_ms),
    }
}

fn compute_misfire_plan_for_schedule(
    now_ms: u64,
    next_fire_at_ms: u64,
    schedule: &RoutineSchedule,
    timezone: &str,
    policy: &RoutineMisfirePolicy,
) -> (u32, u64) {
    match schedule {
        RoutineSchedule::IntervalSeconds { .. } => {
            let Some(interval_ms) = routine_interval_ms(schedule) else {
                return (0, next_fire_at_ms);
            };
            compute_misfire_plan(now_ms, next_fire_at_ms, interval_ms, policy)
        }
        RoutineSchedule::Cron { expression } => {
            let aligned_next = next_cron_fire_at_ms(expression, timezone, now_ms)
                .unwrap_or_else(|| now_ms.saturating_add(60_000));
            match policy {
                RoutineMisfirePolicy::Skip => (0, aligned_next),
                RoutineMisfirePolicy::RunOnce => (1, aligned_next),
                RoutineMisfirePolicy::CatchUp { max_runs } => {
                    let mut count = 0u32;
                    let mut cursor = next_fire_at_ms;
                    while cursor <= now_ms && count < *max_runs {
                        count = count.saturating_add(1);
                        let Some(next) = next_cron_fire_at_ms(expression, timezone, cursor) else {
                            break;
                        };
                        if next <= cursor {
                            break;
                        }
                        cursor = next;
                    }
                    (count, aligned_next)
                }
            }
        }
    }
}

fn compute_misfire_plan(
    now_ms: u64,
    next_fire_at_ms: u64,
    interval_ms: u64,
    policy: &RoutineMisfirePolicy,
) -> (u32, u64) {
    if now_ms < next_fire_at_ms || interval_ms == 0 {
        return (0, next_fire_at_ms);
    }
    let missed = ((now_ms.saturating_sub(next_fire_at_ms)) / interval_ms) + 1;
    let aligned_next = next_fire_at_ms.saturating_add(missed.saturating_mul(interval_ms));
    match policy {
        RoutineMisfirePolicy::Skip => (0, aligned_next),
        RoutineMisfirePolicy::RunOnce => (1, aligned_next),
        RoutineMisfirePolicy::CatchUp { max_runs } => {
            let count = missed.min(u64::from(*max_runs)) as u32;
            (count, aligned_next)
        }
    }
}

fn auto_generated_agent_name(agent_id: &str) -> String {
    let names = [
        "Maple", "Cinder", "Rivet", "Comet", "Atlas", "Juniper", "Quartz", "Beacon",
    ];
    let digest = Sha256::digest(agent_id.as_bytes());
    let idx = usize::from(digest[0]) % names.len();
    format!("{}-{:02x}", names[idx], digest[1])
}

fn schedule_from_automation_v2(schedule: &AutomationV2Schedule) -> Option<RoutineSchedule> {
    match schedule.schedule_type {
        AutomationV2ScheduleType::Manual => None,
        AutomationV2ScheduleType::Interval => Some(RoutineSchedule::IntervalSeconds {
            seconds: schedule.interval_seconds.unwrap_or(60),
        }),
        AutomationV2ScheduleType::Cron => Some(RoutineSchedule::Cron {
            expression: schedule.cron_expression.clone().unwrap_or_default(),
        }),
    }
}

fn automation_schedule_next_fire_at_ms(
    schedule: &AutomationV2Schedule,
    from_ms: u64,
) -> Option<u64> {
    let routine_schedule = schedule_from_automation_v2(schedule)?;
    compute_next_schedule_fire_at_ms(&routine_schedule, &schedule.timezone, from_ms)
}

fn automation_schedule_due_count(
    schedule: &AutomationV2Schedule,
    now_ms: u64,
    next_fire_at_ms: u64,
) -> u32 {
    let Some(routine_schedule) = schedule_from_automation_v2(schedule) else {
        return 0;
    };
    let (count, _) = compute_misfire_plan_for_schedule(
        now_ms,
        next_fire_at_ms,
        &routine_schedule,
        &schedule.timezone,
        &schedule.misfire_policy,
    );
    count.max(1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutineExecutionDecision {
    Allowed,
    RequiresApproval { reason: String },
    Blocked { reason: String },
}

pub fn routine_uses_external_integrations(routine: &RoutineSpec) -> bool {
    let entrypoint = routine.entrypoint.to_ascii_lowercase();
    if entrypoint.starts_with("connector.")
        || entrypoint.starts_with("integration.")
        || entrypoint.contains("external")
    {
        return true;
    }
    routine
        .args
        .get("uses_external_integrations")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || routine
            .args
            .get("connector_id")
            .and_then(|v| v.as_str())
            .is_some()
}

pub fn evaluate_routine_execution_policy(
    routine: &RoutineSpec,
    trigger_type: &str,
) -> RoutineExecutionDecision {
    if !routine_uses_external_integrations(routine) {
        return RoutineExecutionDecision::Allowed;
    }
    if !routine.external_integrations_allowed {
        return RoutineExecutionDecision::Blocked {
            reason: "external integrations are disabled by policy".to_string(),
        };
    }
    if routine.requires_approval {
        return RoutineExecutionDecision::RequiresApproval {
            reason: format!(
                "manual approval required before external side effects ({})",
                trigger_type
            ),
        };
    }
    RoutineExecutionDecision::Allowed
}

fn is_valid_resource_key(key: &str) -> bool {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == "swarm.active_tasks" {
        return true;
    }
    let allowed_prefix = ["run/", "mission/", "project/", "team/"];
    if !allowed_prefix
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return false;
    }
    !trimmed.contains("//")
}

impl Deref for AppState {
    type Target = RuntimeState;

    fn deref(&self) -> &Self::Target {
        self.runtime
            .get()
            .expect("runtime accessed before startup completion")
    }
}

#[derive(Clone)]
struct ServerPromptContextHook {
    state: AppState,
}

impl ServerPromptContextHook {
    fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn open_memory_db(&self) -> Option<MemoryDatabase> {
        let paths = resolve_shared_paths().ok()?;
        MemoryDatabase::new(&paths.memory_db_path).await.ok()
    }

    async fn open_memory_manager(&self) -> Option<tandem_memory::MemoryManager> {
        let paths = resolve_shared_paths().ok()?;
        tandem_memory::MemoryManager::new(&paths.memory_db_path)
            .await
            .ok()
    }

    fn hash_query(input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn build_memory_block(hits: &[tandem_memory::types::GlobalMemorySearchHit]) -> String {
        let mut out = vec!["<memory_context>".to_string()];
        let mut used = 0usize;
        for hit in hits {
            let text = hit
                .record
                .content
                .split_whitespace()
                .take(60)
                .collect::<Vec<_>>()
                .join(" ");
            let line = format!(
                "- [{:.3}] {} (source={}, run={})",
                hit.score, text, hit.record.source_type, hit.record.run_id
            );
            used = used.saturating_add(line.len());
            if used > 2200 {
                break;
            }
            out.push(line);
        }
        out.push("</memory_context>".to_string());
        out.join("\n")
    }

    fn extract_docs_source_url(chunk: &tandem_memory::types::MemoryChunk) -> Option<String> {
        chunk
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("source_url"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
    }

    fn extract_docs_relative_path(chunk: &tandem_memory::types::MemoryChunk) -> String {
        if let Some(path) = chunk
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("relative_path"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return path.to_string();
        }
        chunk
            .source
            .strip_prefix("guide_docs:")
            .unwrap_or(chunk.source.as_str())
            .to_string()
    }

    fn build_docs_memory_block(hits: &[tandem_memory::types::MemorySearchResult]) -> String {
        let mut out = vec!["<docs_context>".to_string()];
        let mut used = 0usize;
        for hit in hits {
            let url = Self::extract_docs_source_url(&hit.chunk).unwrap_or_default();
            let path = Self::extract_docs_relative_path(&hit.chunk);
            let text = hit
                .chunk
                .content
                .split_whitespace()
                .take(70)
                .collect::<Vec<_>>()
                .join(" ");
            let line = format!(
                "- [{:.3}] {} (doc_path={}, source_url={})",
                hit.similarity, text, path, url
            );
            used = used.saturating_add(line.len());
            if used > 2800 {
                break;
            }
            out.push(line);
        }
        out.push("</docs_context>".to_string());
        out.join("\n")
    }

    async fn search_embedded_docs(
        &self,
        query: &str,
        limit: usize,
    ) -> Vec<tandem_memory::types::MemorySearchResult> {
        let Some(manager) = self.open_memory_manager().await else {
            return Vec::new();
        };
        let search_limit = (limit.saturating_mul(3)).clamp(6, 36) as i64;
        manager
            .search(
                query,
                Some(MemoryTier::Global),
                None,
                None,
                Some(search_limit),
            )
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|hit| hit.chunk.source.starts_with("guide_docs:"))
            .take(limit)
            .collect()
    }

    fn should_skip_memory_injection(query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return true;
        }
        let lower = trimmed.to_ascii_lowercase();
        let social = [
            "hi",
            "hello",
            "hey",
            "thanks",
            "thank you",
            "ok",
            "okay",
            "cool",
            "nice",
            "yo",
            "good morning",
            "good afternoon",
            "good evening",
        ];
        lower.len() <= 32 && social.contains(&lower.as_str())
    }

    fn personality_preset_text(preset: &str) -> &'static str {
        match preset {
            "concise" => {
                "Default style: concise and high-signal. Prefer short direct responses unless detail is requested."
            }
            "friendly" => {
                "Default style: friendly and supportive while staying technically rigorous and concrete."
            }
            "mentor" => {
                "Default style: mentor-like. Explain decisions and tradeoffs clearly when complexity is non-trivial."
            }
            "critical" => {
                "Default style: critical and risk-first. Surface failure modes and assumptions early."
            }
            _ => {
                "Default style: balanced, pragmatic, and factual. Focus on concrete outcomes and actionable guidance."
            }
        }
    }

    fn resolve_identity_block(config: &Value, agent_name: Option<&str>) -> Option<String> {
        let allow_agent_override = agent_name
            .map(|name| !matches!(name, "compaction" | "title" | "summary"))
            .unwrap_or(false);
        let legacy_bot_name = config
            .get("bot_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let bot_name = config
            .get("identity")
            .and_then(|identity| identity.get("bot"))
            .and_then(|bot| bot.get("canonical_name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .or(legacy_bot_name)
            .unwrap_or("Tandem");

        let default_profile = config
            .get("identity")
            .and_then(|identity| identity.get("personality"))
            .and_then(|personality| personality.get("default"));
        let default_preset = default_profile
            .and_then(|profile| profile.get("preset"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("balanced");
        let default_custom = default_profile
            .and_then(|profile| profile.get("custom_instructions"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string);
        let legacy_persona = config
            .get("persona")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string);

        let per_agent_profile = if allow_agent_override {
            agent_name.and_then(|name| {
                config
                    .get("identity")
                    .and_then(|identity| identity.get("personality"))
                    .and_then(|personality| personality.get("per_agent"))
                    .and_then(|per_agent| per_agent.get(name))
            })
        } else {
            None
        };
        let preset = per_agent_profile
            .and_then(|profile| profile.get("preset"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(default_preset);
        let custom = per_agent_profile
            .and_then(|profile| profile.get("custom_instructions"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .or(default_custom)
            .or(legacy_persona);

        let mut lines = vec![
            format!("You are {bot_name}, an AI assistant."),
            Self::personality_preset_text(preset).to_string(),
        ];
        if let Some(custom) = custom {
            lines.push(format!("Additional personality instructions: {custom}"));
        }
        Some(lines.join("\n"))
    }
}

impl PromptContextHook for ServerPromptContextHook {
    fn augment_provider_messages(
        &self,
        ctx: PromptContextHookContext,
        mut messages: Vec<ChatMessage>,
    ) -> BoxFuture<'static, anyhow::Result<Vec<ChatMessage>>> {
        let this = self.clone();
        Box::pin(async move {
            // Startup can invoke prompt plumbing before RuntimeState is installed.
            // Never panic from context hooks; fail-open and continue without augmentation.
            if !this.state.is_ready() {
                return Ok(messages);
            }
            let run = this.state.run_registry.get(&ctx.session_id).await;
            let Some(run) = run else {
                return Ok(messages);
            };
            let config = this.state.config.get_effective_value().await;
            if let Some(identity_block) =
                Self::resolve_identity_block(&config, run.agent_profile.as_deref())
            {
                messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: identity_block,
                    attachments: Vec::new(),
                });
            }
            let run_id = run.run_id;
            let user_id = run.client_id.unwrap_or_else(|| "default".to_string());
            let query = messages
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| m.content.clone())
                .unwrap_or_default();
            if query.trim().is_empty() {
                return Ok(messages);
            }
            if Self::should_skip_memory_injection(&query) {
                return Ok(messages);
            }

            let docs_hits = this.search_embedded_docs(&query, 6).await;
            if !docs_hits.is_empty() {
                let docs_block = Self::build_docs_memory_block(&docs_hits);
                messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: docs_block.clone(),
                    attachments: Vec::new(),
                });
                this.state.event_bus.publish(EngineEvent::new(
                    "memory.docs.context.injected",
                    json!({
                        "runID": run_id,
                        "sessionID": ctx.session_id,
                        "messageID": ctx.message_id,
                        "iteration": ctx.iteration,
                        "count": docs_hits.len(),
                        "tokenSizeApprox": docs_block.split_whitespace().count(),
                        "sourcePrefix": "guide_docs:"
                    }),
                ));
                return Ok(messages);
            }

            let Some(db) = this.open_memory_db().await else {
                return Ok(messages);
            };
            let started = now_ms();
            let hits = db
                .search_global_memory(&user_id, &query, 8, None, None, None)
                .await
                .unwrap_or_default();
            let latency_ms = now_ms().saturating_sub(started);
            let scores = hits.iter().map(|h| h.score).collect::<Vec<_>>();
            this.state.event_bus.publish(EngineEvent::new(
                "memory.search.performed",
                json!({
                    "runID": run_id,
                    "sessionID": ctx.session_id,
                    "messageID": ctx.message_id,
                    "providerID": ctx.provider_id,
                    "modelID": ctx.model_id,
                    "iteration": ctx.iteration,
                    "queryHash": Self::hash_query(&query),
                    "resultCount": hits.len(),
                    "scoreMin": scores.iter().copied().reduce(f64::min),
                    "scoreMax": scores.iter().copied().reduce(f64::max),
                    "scores": scores,
                    "latencyMs": latency_ms,
                    "sources": hits.iter().map(|h| h.record.source_type.clone()).collect::<Vec<_>>(),
                }),
            ));

            if hits.is_empty() {
                return Ok(messages);
            }

            let memory_block = Self::build_memory_block(&hits);
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: memory_block.clone(),
                attachments: Vec::new(),
            });
            this.state.event_bus.publish(EngineEvent::new(
                "memory.context.injected",
                json!({
                    "runID": run_id,
                    "sessionID": ctx.session_id,
                    "messageID": ctx.message_id,
                    "iteration": ctx.iteration,
                    "count": hits.len(),
                    "tokenSizeApprox": memory_block.split_whitespace().count(),
                }),
            ));
            Ok(messages)
        })
    }
}

fn extract_event_session_id(properties: &Value) -> Option<String> {
    properties
        .get("sessionID")
        .or_else(|| properties.get("sessionId"))
        .or_else(|| properties.get("id"))
        .or_else(|| {
            properties
                .get("part")
                .and_then(|part| part.get("sessionID"))
        })
        .or_else(|| {
            properties
                .get("part")
                .and_then(|part| part.get("sessionId"))
        })
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_event_run_id(properties: &Value) -> Option<String> {
    properties
        .get("runID")
        .or_else(|| properties.get("run_id"))
        .or_else(|| properties.get("part").and_then(|part| part.get("runID")))
        .or_else(|| properties.get("part").and_then(|part| part.get("run_id")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_persistable_tool_part(properties: &Value) -> Option<(String, MessagePart)> {
    let part = properties.get("part")?;
    let part_type = part
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if part_type != "tool" && part_type != "tool-invocation" && part_type != "tool-result" {
        return None;
    }
    let tool = part.get("tool").and_then(|v| v.as_str())?.to_string();
    let message_id = part
        .get("messageID")
        .or_else(|| part.get("message_id"))
        .and_then(|v| v.as_str())?
        .to_string();
    let mut args = part.get("args").cloned().unwrap_or_else(|| json!({}));
    if args.is_null() || args.as_object().is_some_and(|value| value.is_empty()) {
        if let Some(preview) = properties
            .get("toolCallDelta")
            .and_then(|delta| delta.get("parsedArgsPreview"))
            .cloned()
        {
            let preview_nonempty = !preview.is_null()
                && !preview.as_object().is_some_and(|value| value.is_empty())
                && !preview
                    .as_str()
                    .map(|value| value.trim().is_empty())
                    .unwrap_or(false);
            if preview_nonempty {
                args = preview;
            }
        }
    }
    if tool == "write" && (args.is_null() || args.as_object().is_some_and(|value| value.is_empty()))
    {
        tracing::info!(
            message_id = %message_id,
            has_tool_call_delta = properties.get("toolCallDelta").is_some(),
            part_state = %part.get("state").and_then(|v| v.as_str()).unwrap_or(""),
            has_result = part.get("result").is_some(),
            has_error = part.get("error").is_some(),
            "persistable write tool part still has empty args"
        );
    }
    let result = part.get("result").cloned().filter(|value| !value.is_null());
    let error = part
        .get("error")
        .and_then(|v| v.as_str())
        .map(|value| value.to_string());
    Some((
        message_id,
        MessagePart::ToolInvocation {
            tool,
            args,
            result,
            error,
        },
    ))
}

fn derive_status_index_update(event: &EngineEvent) -> Option<StatusIndexUpdate> {
    let session_id = extract_event_session_id(&event.properties)?;
    let run_id = extract_event_run_id(&event.properties);
    let key = format!("run/{session_id}/status");

    let mut base = serde_json::Map::new();
    base.insert("sessionID".to_string(), Value::String(session_id));
    if let Some(run_id) = run_id {
        base.insert("runID".to_string(), Value::String(run_id));
    }

    match event.event_type.as_str() {
        "session.run.started" => {
            base.insert("state".to_string(), Value::String("running".to_string()));
            base.insert("phase".to_string(), Value::String("run".to_string()));
            base.insert(
                "eventType".to_string(),
                Value::String("session.run.started".to_string()),
            );
            Some(StatusIndexUpdate {
                key,
                value: Value::Object(base),
            })
        }
        "session.run.finished" => {
            base.insert("state".to_string(), Value::String("finished".to_string()));
            base.insert("phase".to_string(), Value::String("run".to_string()));
            if let Some(status) = event.properties.get("status").and_then(|v| v.as_str()) {
                base.insert("result".to_string(), Value::String(status.to_string()));
            }
            base.insert(
                "eventType".to_string(),
                Value::String("session.run.finished".to_string()),
            );
            Some(StatusIndexUpdate {
                key,
                value: Value::Object(base),
            })
        }
        "message.part.updated" => {
            let part_type = event
                .properties
                .get("part")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())?;
            let part_state = event
                .properties
                .get("part")
                .and_then(|v| v.get("state"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (phase, tool_active) = match (part_type, part_state) {
                ("tool-invocation", _) | ("tool", "running") | ("tool", "") => ("tool", true),
                ("tool-result", _) | ("tool", "completed") | ("tool", "failed") => ("run", false),
                _ => return None,
            };
            base.insert("state".to_string(), Value::String("running".to_string()));
            base.insert("phase".to_string(), Value::String(phase.to_string()));
            base.insert("toolActive".to_string(), Value::Bool(tool_active));
            if let Some(tool_name) = event
                .properties
                .get("part")
                .and_then(|v| v.get("tool"))
                .and_then(|v| v.as_str())
            {
                base.insert("tool".to_string(), Value::String(tool_name.to_string()));
            }
            base.insert(
                "eventType".to_string(),
                Value::String("message.part.updated".to_string()),
            );
            Some(StatusIndexUpdate {
                key,
                value: Value::Object(base),
            })
        }
        _ => None,
    }
}

pub async fn run_session_part_persister(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("session part persister: skipped because runtime did not become ready");
        return;
    }
    let Some(mut rx) = state.event_bus.take_session_part_receiver() else {
        tracing::warn!("session part persister: skipped because receiver was already taken");
        return;
    };
    while let Some(event) = rx.recv().await {
        if event.event_type != "message.part.updated" {
            continue;
        }
        // Streaming tool-call previews are useful for the live UI, but persistence
        // should store the finalized invocation/result events to avoid duplicating
        // one tool part per streamed args delta.
        if event.properties.get("toolCallDelta").is_some() {
            continue;
        }
        let Some(session_id) = extract_event_session_id(&event.properties) else {
            continue;
        };
        let Some((message_id, part)) = extract_persistable_tool_part(&event.properties) else {
            continue;
        };
        if let Err(error) = state
            .storage
            .append_message_part(&session_id, &message_id, part)
            .await
        {
            tracing::warn!(
                "session part persister failed for session={} message={}: {error:#}",
                session_id,
                message_id
            );
        }
    }
}

pub async fn run_status_indexer(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("status indexer: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if let Some(update) = derive_status_index_update(&event) {
                    if let Err(error) = state
                        .put_shared_resource(
                            update.key,
                            update.value,
                            None,
                            "system.status_indexer".to_string(),
                            None,
                        )
                        .await
                    {
                        tracing::warn!("status indexer failed to persist update: {error:?}");
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub async fn run_agent_team_supervisor(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("agent team supervisor: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                state.agent_teams.handle_engine_event(&state, &event).await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub async fn run_bug_monitor(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("bug monitor: skipped because runtime did not become ready");
        return;
    }
    state
        .update_bug_monitor_runtime_status(|runtime| {
            runtime.monitoring_active = false;
            runtime.last_runtime_error = None;
        })
        .await;
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if !is_bug_monitor_candidate_event(&event) {
                    continue;
                }
                let status = state.bug_monitor_status().await;
                if !status.config.enabled || status.config.paused || !status.readiness.repo_valid {
                    state
                        .update_bug_monitor_runtime_status(|runtime| {
                            runtime.monitoring_active = status.config.enabled
                                && !status.config.paused
                                && status.readiness.repo_valid;
                            runtime.paused = status.config.paused;
                            runtime.last_runtime_error = status.last_error.clone();
                        })
                        .await;
                    continue;
                }
                match process_bug_monitor_event(&state, &event, &status.config).await {
                    Ok(incident) => {
                        state
                            .update_bug_monitor_runtime_status(|runtime| {
                                runtime.monitoring_active = true;
                                runtime.paused = status.config.paused;
                                runtime.last_processed_at_ms = Some(now_ms());
                                runtime.last_incident_event_type =
                                    Some(incident.event_type.clone());
                                runtime.last_runtime_error = None;
                            })
                            .await;
                    }
                    Err(error) => {
                        let detail = truncate_text(&error.to_string(), 500);
                        state
                            .update_bug_monitor_runtime_status(|runtime| {
                                runtime.monitoring_active = true;
                                runtime.paused = status.config.paused;
                                runtime.last_processed_at_ms = Some(now_ms());
                                runtime.last_incident_event_type = Some(event.event_type.clone());
                                runtime.last_runtime_error = Some(detail.clone());
                            })
                            .await;
                        state.event_bus.publish(EngineEvent::new(
                            "bug_monitor.error",
                            serde_json::json!({
                                "eventType": event.event_type,
                                "detail": detail,
                            }),
                        ));
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                state
                    .update_bug_monitor_runtime_status(|runtime| {
                        runtime.last_runtime_error =
                            Some(format!("Bug monitor lagged and dropped {count} events."));
                    })
                    .await;
            }
        }
    }
}

pub async fn run_usage_aggregator(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("usage aggregator: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if event.event_type != "provider.usage" {
                    continue;
                }
                let session_id = event
                    .properties
                    .get("sessionID")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if session_id.is_empty() {
                    continue;
                }
                let prompt_tokens = event
                    .properties
                    .get("promptTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let completion_tokens = event
                    .properties
                    .get("completionTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let total_tokens = event
                    .properties
                    .get("totalTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(prompt_tokens.saturating_add(completion_tokens));
                state
                    .apply_provider_usage_to_runs(
                        session_id,
                        prompt_tokens,
                        completion_tokens,
                        total_tokens,
                    )
                    .await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

fn is_bug_monitor_candidate_event(event: &EngineEvent) -> bool {
    if event.event_type.starts_with("bug_monitor.") {
        return false;
    }
    matches!(
        event.event_type.as_str(),
        "context.task.failed" | "workflow.run.failed" | "routine.run.failed" | "session.error"
    )
}

async fn process_bug_monitor_event(
    state: &AppState,
    event: &EngineEvent,
    config: &BugMonitorConfig,
) -> anyhow::Result<BugMonitorIncidentRecord> {
    let submission = build_bug_monitor_submission_from_event(state, config, event).await?;
    let duplicate_matches = crate::http::bug_monitor::bug_monitor_failure_pattern_matches(
        state,
        submission.repo.as_deref().unwrap_or_default(),
        submission.fingerprint.as_deref().unwrap_or_default(),
        submission.title.as_deref(),
        submission.detail.as_deref(),
        &submission.excerpt,
        3,
    )
    .await;
    let fingerprint = submission
        .fingerprint
        .clone()
        .ok_or_else(|| anyhow::anyhow!("bug monitor submission fingerprint missing"))?;
    let default_workspace_root = state.workspace_index.snapshot().await.root;
    let workspace_root = config
        .workspace_root
        .clone()
        .unwrap_or(default_workspace_root);
    let now = now_ms();

    let existing = state
        .bug_monitor_incidents
        .read()
        .await
        .values()
        .find(|row| row.fingerprint == fingerprint)
        .cloned();

    let mut incident = if let Some(mut row) = existing {
        row.occurrence_count = row.occurrence_count.saturating_add(1);
        row.updated_at_ms = now;
        row.last_seen_at_ms = Some(now);
        if row.excerpt.is_empty() {
            row.excerpt = submission.excerpt.clone();
        }
        row
    } else {
        BugMonitorIncidentRecord {
            incident_id: format!("failure-incident-{}", uuid::Uuid::new_v4().simple()),
            fingerprint: fingerprint.clone(),
            event_type: event.event_type.clone(),
            status: "queued".to_string(),
            repo: submission.repo.clone().unwrap_or_default(),
            workspace_root,
            title: submission
                .title
                .clone()
                .unwrap_or_else(|| format!("Failure detected in {}", event.event_type)),
            detail: submission.detail.clone(),
            excerpt: submission.excerpt.clone(),
            source: submission.source.clone(),
            run_id: submission.run_id.clone(),
            session_id: submission.session_id.clone(),
            correlation_id: submission.correlation_id.clone(),
            component: submission.component.clone(),
            level: submission.level.clone(),
            occurrence_count: 1,
            created_at_ms: now,
            updated_at_ms: now,
            last_seen_at_ms: Some(now),
            draft_id: None,
            triage_run_id: None,
            last_error: None,
            duplicate_summary: None,
            duplicate_matches: None,
            event_payload: Some(event.properties.clone()),
        }
    };
    state.put_bug_monitor_incident(incident.clone()).await?;

    if !duplicate_matches.is_empty() {
        incident.status = "duplicate_suppressed".to_string();
        let duplicate_summary =
            crate::http::bug_monitor::build_bug_monitor_duplicate_summary(&duplicate_matches);
        incident.duplicate_summary = Some(duplicate_summary.clone());
        incident.duplicate_matches = Some(duplicate_matches.clone());
        incident.updated_at_ms = now_ms();
        state.put_bug_monitor_incident(incident.clone()).await?;
        state.event_bus.publish(EngineEvent::new(
            "bug_monitor.incident.duplicate_suppressed",
            serde_json::json!({
                "incident_id": incident.incident_id,
                "fingerprint": incident.fingerprint,
                "eventType": incident.event_type,
                "status": incident.status,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
            }),
        ));
        return Ok(incident);
    }

    let draft = match state.submit_bug_monitor_draft(submission).await {
        Ok(draft) => draft,
        Err(error) => {
            incident.status = "draft_failed".to_string();
            incident.last_error = Some(truncate_text(&error.to_string(), 500));
            incident.updated_at_ms = now_ms();
            state.put_bug_monitor_incident(incident.clone()).await?;
            state.event_bus.publish(EngineEvent::new(
                "bug_monitor.incident.detected",
                serde_json::json!({
                    "incident_id": incident.incident_id,
                    "fingerprint": incident.fingerprint,
                    "eventType": incident.event_type,
                    "draft_id": incident.draft_id,
                    "triage_run_id": incident.triage_run_id,
                    "status": incident.status,
                    "detail": incident.last_error,
                }),
            ));
            return Ok(incident);
        }
    };
    incident.draft_id = Some(draft.draft_id.clone());
    incident.status = "draft_created".to_string();
    state.put_bug_monitor_incident(incident.clone()).await?;

    match crate::http::bug_monitor::ensure_bug_monitor_triage_run(
        state.clone(),
        &draft.draft_id,
        true,
    )
    .await
    {
        Ok((updated_draft, _run_id, _deduped)) => {
            incident.triage_run_id = updated_draft.triage_run_id.clone();
            if incident.triage_run_id.is_some() {
                incident.status = "triage_queued".to_string();
            }
            incident.last_error = None;
        }
        Err(error) => {
            incident.status = "draft_created".to_string();
            incident.last_error = Some(truncate_text(&error.to_string(), 500));
        }
    }

    if let Some(draft_id) = incident.draft_id.clone() {
        let latest_draft = state
            .get_bug_monitor_draft(&draft_id)
            .await
            .unwrap_or(draft.clone());
        match crate::bug_monitor_github::publish_draft(
            state,
            &draft_id,
            Some(&incident.incident_id),
            crate::bug_monitor_github::PublishMode::Auto,
        )
        .await
        {
            Ok(outcome) => {
                incident.status = outcome.action;
                incident.last_error = None;
            }
            Err(error) => {
                let detail = truncate_text(&error.to_string(), 500);
                incident.last_error = Some(detail.clone());
                let mut failed_draft = latest_draft;
                failed_draft.status = "github_post_failed".to_string();
                failed_draft.github_status = Some("github_post_failed".to_string());
                failed_draft.last_post_error = Some(detail.clone());
                let evidence_digest = failed_draft.evidence_digest.clone();
                let _ = state.put_bug_monitor_draft(failed_draft.clone()).await;
                let _ = crate::bug_monitor_github::record_post_failure(
                    state,
                    &failed_draft,
                    Some(&incident.incident_id),
                    "auto_post",
                    evidence_digest.as_deref(),
                    &detail,
                )
                .await;
            }
        }
    }

    incident.updated_at_ms = now_ms();
    state.put_bug_monitor_incident(incident.clone()).await?;
    state.event_bus.publish(EngineEvent::new(
        "bug_monitor.incident.detected",
        serde_json::json!({
            "incident_id": incident.incident_id,
            "fingerprint": incident.fingerprint,
            "eventType": incident.event_type,
            "draft_id": incident.draft_id,
            "triage_run_id": incident.triage_run_id,
            "status": incident.status,
        }),
    ));
    Ok(incident)
}

async fn build_bug_monitor_submission_from_event(
    state: &AppState,
    config: &BugMonitorConfig,
    event: &EngineEvent,
) -> anyhow::Result<BugMonitorSubmission> {
    let repo = config
        .repo
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor repo is not configured"))?;
    let default_workspace_root = state.workspace_index.snapshot().await.root;
    let workspace_root = config
        .workspace_root
        .clone()
        .unwrap_or(default_workspace_root);
    let reason = first_string(
        &event.properties,
        &["reason", "error", "detail", "message", "summary"],
    );
    let run_id = first_string(&event.properties, &["runID", "run_id"]);
    let session_id = first_string(&event.properties, &["sessionID", "session_id"]);
    let correlation_id = first_string(
        &event.properties,
        &["correlationID", "correlation_id", "commandID", "command_id"],
    );
    let component = first_string(
        &event.properties,
        &[
            "component",
            "routineID",
            "routine_id",
            "workflowID",
            "workflow_id",
            "task",
            "title",
        ],
    );
    let mut excerpt = collect_bug_monitor_excerpt(state, &event.properties).await;
    if excerpt.is_empty() {
        if let Some(reason) = reason.as_ref() {
            excerpt.push(reason.clone());
        }
    }
    let serialized = serde_json::to_string(&event.properties).unwrap_or_default();
    let fingerprint = sha256_hex(&[
        repo.as_str(),
        workspace_root.as_str(),
        event.event_type.as_str(),
        reason.as_deref().unwrap_or(""),
        run_id.as_deref().unwrap_or(""),
        session_id.as_deref().unwrap_or(""),
        correlation_id.as_deref().unwrap_or(""),
        component.as_deref().unwrap_or(""),
        serialized.as_str(),
    ]);
    let title = if let Some(component) = component.as_ref() {
        format!("{} failure in {}", event.event_type, component)
    } else {
        format!("{} detected", event.event_type)
    };
    let mut detail_lines = vec![
        format!("event_type: {}", event.event_type),
        format!("workspace_root: {}", workspace_root),
    ];
    if let Some(reason) = reason.as_ref() {
        detail_lines.push(format!("reason: {reason}"));
    }
    if let Some(run_id) = run_id.as_ref() {
        detail_lines.push(format!("run_id: {run_id}"));
    }
    if let Some(session_id) = session_id.as_ref() {
        detail_lines.push(format!("session_id: {session_id}"));
    }
    if let Some(correlation_id) = correlation_id.as_ref() {
        detail_lines.push(format!("correlation_id: {correlation_id}"));
    }
    if let Some(component) = component.as_ref() {
        detail_lines.push(format!("component: {component}"));
    }
    if !serialized.trim().is_empty() {
        detail_lines.push(String::new());
        detail_lines.push("payload:".to_string());
        detail_lines.push(truncate_text(&serialized, 2_000));
    }

    Ok(BugMonitorSubmission {
        repo: Some(repo),
        title: Some(title),
        detail: Some(detail_lines.join("\n")),
        source: Some("tandem_events".to_string()),
        run_id,
        session_id,
        correlation_id,
        file_name: None,
        process: Some("tandem-engine".to_string()),
        component,
        event: Some(event.event_type.clone()),
        level: Some("error".to_string()),
        excerpt,
        fingerprint: Some(fingerprint),
    })
}

async fn collect_bug_monitor_excerpt(state: &AppState, properties: &Value) -> Vec<String> {
    let mut excerpt = Vec::new();
    if let Some(reason) = first_string(properties, &["reason", "error", "detail", "message"]) {
        excerpt.push(reason);
    }
    if let Some(title) = first_string(properties, &["title", "task"]) {
        if !excerpt.iter().any(|row| row == &title) {
            excerpt.push(title);
        }
    }
    let logs = state.logs.read().await;
    for entry in logs.iter().rev().take(3) {
        if let Some(message) = entry.get("message").and_then(|row| row.as_str()) {
            excerpt.push(truncate_text(message, 240));
        }
    }
    excerpt.truncate(8);
    excerpt
}

fn first_string(properties: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = properties.get(*key).and_then(|row| row.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

pub async fn run_routine_scheduler(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let now = now_ms();
        let plans = state.evaluate_routine_misfires(now).await;
        for plan in plans {
            let Some(routine) = state.get_routine(&plan.routine_id).await else {
                continue;
            };
            match evaluate_routine_execution_policy(&routine, "scheduled") {
                RoutineExecutionDecision::Allowed => {
                    let _ = state.mark_routine_fired(&plan.routine_id, now).await;
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::Queued,
                            None,
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.routine_id.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "queued".to_string(),
                            detail: None,
                        })
                        .await;
                    state.event_bus.publish(EngineEvent::new(
                        "routine.fired",
                        serde_json::json!({
                            "routineID": plan.routine_id,
                            "runID": run.run_id,
                            "runCount": plan.run_count,
                            "scheduledAtMs": plan.scheduled_at_ms,
                            "nextFireAtMs": plan.next_fire_at_ms,
                        }),
                    ));
                    state.event_bus.publish(EngineEvent::new(
                        "routine.run.created",
                        serde_json::json!({
                            "run": run,
                        }),
                    ));
                }
                RoutineExecutionDecision::RequiresApproval { reason } => {
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::PendingApproval,
                            Some(reason.clone()),
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.routine_id.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "pending_approval".to_string(),
                            detail: Some(reason.clone()),
                        })
                        .await;
                    state.event_bus.publish(EngineEvent::new(
                        "routine.approval_required",
                        serde_json::json!({
                            "routineID": plan.routine_id,
                            "runID": run.run_id,
                            "runCount": plan.run_count,
                            "triggerType": "scheduled",
                            "reason": reason,
                        }),
                    ));
                    state.event_bus.publish(EngineEvent::new(
                        "routine.run.created",
                        serde_json::json!({
                            "run": run,
                        }),
                    ));
                }
                RoutineExecutionDecision::Blocked { reason } => {
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::BlockedPolicy,
                            Some(reason.clone()),
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.routine_id.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "blocked_policy".to_string(),
                            detail: Some(reason.clone()),
                        })
                        .await;
                    state.event_bus.publish(EngineEvent::new(
                        "routine.blocked",
                        serde_json::json!({
                            "routineID": plan.routine_id,
                            "runID": run.run_id,
                            "runCount": plan.run_count,
                            "triggerType": "scheduled",
                            "reason": reason,
                        }),
                    ));
                    state.event_bus.publish(EngineEvent::new(
                        "routine.run.created",
                        serde_json::json!({
                            "run": run,
                        }),
                    ));
                }
            }
        }
    }
}

pub async fn run_routine_executor(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let Some(run) = state.claim_next_queued_routine_run().await else {
            continue;
        };

        state.event_bus.publish(EngineEvent::new(
            "routine.run.started",
            serde_json::json!({
                "runID": run.run_id,
                "routineID": run.routine_id,
                "triggerType": run.trigger_type,
                "startedAtMs": now_ms(),
            }),
        ));

        let workspace_root = state.workspace_index.snapshot().await.root;
        let mut session = Session::new(
            Some(format!("Routine {}", run.routine_id)),
            Some(workspace_root.clone()),
        );
        let session_id = session.id.clone();
        session.workspace_root = Some(workspace_root);

        if let Err(error) = state.storage.save_session(session).await {
            let detail = format!("failed to create routine session: {error}");
            let _ = state
                .update_routine_run_status(
                    &run.run_id,
                    RoutineRunStatus::Failed,
                    Some(detail.clone()),
                )
                .await;
            state.event_bus.publish(EngineEvent::new(
                "routine.run.failed",
                serde_json::json!({
                    "runID": run.run_id,
                    "routineID": run.routine_id,
                    "reason": detail,
                }),
            ));
            continue;
        }

        state
            .set_routine_session_policy(
                session_id.clone(),
                run.run_id.clone(),
                run.routine_id.clone(),
                run.allowed_tools.clone(),
            )
            .await;
        state
            .add_active_session_id(&run.run_id, session_id.clone())
            .await;
        state
            .engine_loop
            .set_session_allowed_tools(&session_id, run.allowed_tools.clone())
            .await;
        state
            .engine_loop
            .set_session_auto_approve_permissions(&session_id, true)
            .await;

        let (selected_model, model_source) = resolve_routine_model_spec_for_run(&state, &run).await;
        if let Some(spec) = selected_model.as_ref() {
            state.event_bus.publish(EngineEvent::new(
                "routine.run.model_selected",
                serde_json::json!({
                    "runID": run.run_id,
                    "routineID": run.routine_id,
                    "providerID": spec.provider_id,
                    "modelID": spec.model_id,
                    "source": model_source,
                }),
            ));
        }

        let request = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: build_routine_prompt(&state, &run).await,
            }],
            model: selected_model,
            agent: None,
            tool_mode: None,
            tool_allowlist: None,
            context_mode: None,
            write_required: None,
        };

        let run_result = state
            .engine_loop
            .run_prompt_async_with_context(
                session_id.clone(),
                request,
                Some(format!("routine:{}", run.run_id)),
            )
            .await;

        state.clear_routine_session_policy(&session_id).await;
        state
            .clear_active_session_id(&run.run_id, &session_id)
            .await;
        state
            .engine_loop
            .clear_session_allowed_tools(&session_id)
            .await;
        state
            .engine_loop
            .clear_session_auto_approve_permissions(&session_id)
            .await;

        match run_result {
            Ok(()) => {
                append_configured_output_artifacts(&state, &run).await;
                let _ = state
                    .update_routine_run_status(
                        &run.run_id,
                        RoutineRunStatus::Completed,
                        Some("routine run completed".to_string()),
                    )
                    .await;
                state.event_bus.publish(EngineEvent::new(
                    "routine.run.completed",
                    serde_json::json!({
                        "runID": run.run_id,
                        "routineID": run.routine_id,
                        "sessionID": session_id,
                        "finishedAtMs": now_ms(),
                    }),
                ));
            }
            Err(error) => {
                if let Some(latest) = state.get_routine_run(&run.run_id).await {
                    if latest.status == RoutineRunStatus::Paused {
                        state.event_bus.publish(EngineEvent::new(
                            "routine.run.paused",
                            serde_json::json!({
                                "runID": run.run_id,
                                "routineID": run.routine_id,
                                "sessionID": session_id,
                                "finishedAtMs": now_ms(),
                            }),
                        ));
                        continue;
                    }
                }
                let detail = truncate_text(&error.to_string(), 500);
                let _ = state
                    .update_routine_run_status(
                        &run.run_id,
                        RoutineRunStatus::Failed,
                        Some(detail.clone()),
                    )
                    .await;
                state.event_bus.publish(EngineEvent::new(
                    "routine.run.failed",
                    serde_json::json!({
                        "runID": run.run_id,
                        "routineID": run.routine_id,
                        "sessionID": session_id,
                        "reason": detail,
                        "finishedAtMs": now_ms(),
                    }),
                ));
            }
        }
    }
}

pub async fn run_automation_v2_scheduler(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let startup = state.startup_snapshot().await;
        if !matches!(startup.status, StartupStatus::Ready) {
            continue;
        }
        let now = now_ms();
        let due = state.evaluate_automation_v2_misfires(now).await;
        for automation_id in due {
            let Some(automation) = state.get_automation_v2(&automation_id).await else {
                continue;
            };
            if let Ok(run) = state
                .create_automation_v2_run(&automation, "scheduled")
                .await
            {
                state.event_bus.publish(EngineEvent::new(
                    "automation.v2.run.created",
                    serde_json::json!({
                        "automationID": automation_id,
                        "run": run,
                        "triggerType": "scheduled",
                    }),
                ));
            }
        }
    }
}

fn build_automation_v2_upstream_inputs(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
) -> anyhow::Result<Vec<Value>> {
    let mut inputs = Vec::new();
    for input_ref in &node.input_refs {
        let Some(output) = run.checkpoint.node_outputs.get(&input_ref.from_step_id) else {
            anyhow::bail!(
                "missing upstream output for `{}` referenced by node `{}`",
                input_ref.from_step_id,
                node.node_id
            );
        };
        inputs.push(json!({
            "alias": input_ref.alias,
            "from_step_id": input_ref.from_step_id,
            "output": output,
        }));
    }
    Ok(inputs)
}

fn render_automation_v2_prompt(
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
) -> String {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("structured_json");
    let mut prompt = format!(
        "Automation ID: {}\nRun ID: {}\nNode ID: {}\nAgent: {}\nObjective: {}\nOutput contract kind: {}",
        automation.automation_id, run_id, node.node_id, agent.display_name, node.objective, contract_kind
    );
    if !upstream_inputs.is_empty() {
        prompt.push_str("\n\nUpstream Inputs:");
        for input in upstream_inputs {
            let alias = input
                .get("alias")
                .and_then(Value::as_str)
                .unwrap_or("input");
            let from_step_id = input
                .get("from_step_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let output = input.get("output").cloned().unwrap_or(Value::Null);
            let rendered =
                serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string());
            prompt.push_str(&format!(
                "\n- {}\n  from_step_id: {}\n  output:\n{}",
                alias,
                from_step_id,
                rendered
                    .lines()
                    .map(|line| format!("    {}", line))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    if node.node_id == "notify_user" || node.objective.to_ascii_lowercase().contains("email") {
        prompt.push_str(
            "\n\nDelivery rules:\n- Prefer inline email body delivery by default.\n- Only include an email attachment when upstream inputs contain a concrete attachment artifact with a non-empty s3key or upload result.\n- Never send an attachment parameter with an empty or null s3key.\n- If no attachment artifact exists, omit the attachment parameter entirely.",
        );
    }
    prompt.push_str(
        "\n\nReturn a concise completion. If you produce structured content, keep it valid JSON inside the response body.",
    );
    prompt
}

fn extract_session_text_output(session: &Session) -> String {
    session
        .messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, MessageRole::Assistant))
        .map(|message| {
            message
                .parts
                .iter()
                .filter_map(|part| match part {
                    MessagePart::Text { text } | MessagePart::Reasoning { text } => {
                        Some(text.as_str())
                    }
                    MessagePart::ToolInvocation { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn wrap_automation_node_output(
    node: &AutomationFlowNode,
    session_id: &str,
    session_text: &str,
) -> Value {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.clone())
        .unwrap_or_else(|| "structured_json".to_string());
    let summary = if session_text.trim().is_empty() {
        format!("Node `{}` completed successfully.", node.node_id)
    } else {
        truncate_text(session_text.trim(), 240)
    };
    let content = match contract_kind.as_str() {
        "report_markdown" | "text_summary" => {
            json!({ "text": session_text.trim(), "session_id": session_id })
        }
        "urls" => json!({ "items": [], "raw_text": session_text.trim(), "session_id": session_id }),
        "citations" => {
            json!({ "items": [], "raw_text": session_text.trim(), "session_id": session_id })
        }
        _ => json!({ "text": session_text.trim(), "session_id": session_id }),
    };
    json!(AutomationNodeOutput {
        contract_kind,
        summary,
        content,
        created_at_ms: now_ms(),
        node_id: node.node_id.clone(),
    })
}

fn automation_node_max_attempts(node: &AutomationFlowNode) -> u32 {
    node.retry_policy
        .as_ref()
        .and_then(|value| value.get("max_attempts"))
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 10) as u32)
        .unwrap_or(3)
}

async fn resolve_automation_v2_workspace_root(
    state: &AppState,
    automation: &AutomationV2Spec,
) -> String {
    if let Some(workspace_root) = automation
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        return workspace_root;
    }
    if let Some(workspace_root) = automation
        .metadata
        .as_ref()
        .and_then(|row| row.get("workspace_root"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        return workspace_root;
    }
    state.workspace_index.snapshot().await.root
}

async fn execute_automation_v2_node(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    agent: &AutomationAgentProfile,
) -> anyhow::Result<Value> {
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{}` not found", run_id))?;
    let upstream_inputs = build_automation_v2_upstream_inputs(&run, node)?;
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    let workspace_path = PathBuf::from(&workspace_root);
    if !workspace_path.exists() {
        anyhow::bail!(
            "workspace_root `{}` for automation `{}` does not exist",
            workspace_root,
            automation.automation_id
        );
    }
    if !workspace_path.is_dir() {
        anyhow::bail!(
            "workspace_root `{}` for automation `{}` is not a directory",
            workspace_root,
            automation.automation_id
        );
    }
    let mut session = Session::new(
        Some(format!(
            "Automation {} / {}",
            automation.automation_id, node.node_id
        )),
        Some(workspace_root.clone()),
    );
    let session_id = session.id.clone();
    session.workspace_root = Some(workspace_root);
    state.storage.save_session(session).await?;

    state.add_automation_v2_session(run_id, &session_id).await;

    let mut allowlist = agent.tool_policy.allowlist.clone();
    if let Some(mcp_tools) = agent.mcp_policy.allowed_tools.as_ref() {
        allowlist.extend(mcp_tools.clone());
    }
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, normalize_allowed_tools(allowlist))
        .await;
    state
        .engine_loop
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    let model = agent
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(parse_model_spec);
    let prompt = render_automation_v2_prompt(automation, run_id, node, agent, &upstream_inputs);
    let req = SendMessageRequest {
        parts: vec![MessagePartInput::Text { text: prompt }],
        model,
        agent: None,
        tool_mode: None,
        tool_allowlist: None,
        context_mode: None,
        write_required: None,
    };
    let result = state
        .engine_loop
        .run_prompt_async_with_context(
            session_id.clone(),
            req,
            Some(format!("automation-v2:{run_id}")),
        )
        .await;

    state
        .engine_loop
        .clear_session_allowed_tools(&session_id)
        .await;
    state
        .engine_loop
        .clear_session_auto_approve_permissions(&session_id)
        .await;
    state.clear_automation_v2_session(run_id, &session_id).await;

    result?;
    let session = state
        .storage
        .get_session(&session_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation session `{}` missing after run", session_id))?;
    let session_text = extract_session_text_output(&session);
    Ok(wrap_automation_node_output(
        node,
        &session_id,
        &session_text,
    ))
}

pub async fn run_automation_v2_executor(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let Some(run) = state.claim_next_queued_automation_v2_run().await else {
            continue;
        };
        let Some(automation) = state.get_automation_v2(&run.automation_id).await else {
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Failed;
                    row.detail = Some("automation not found".to_string());
                })
                .await;
            continue;
        };
        let max_parallel = automation
            .execution
            .max_parallel_agents
            .unwrap_or(1)
            .clamp(1, 16) as usize;

        loop {
            let Some(latest) = state.get_automation_v2_run(&run.run_id).await else {
                break;
            };
            if matches!(
                latest.status,
                AutomationRunStatus::Paused
                    | AutomationRunStatus::Pausing
                    | AutomationRunStatus::Cancelled
                    | AutomationRunStatus::Failed
                    | AutomationRunStatus::Completed
            ) {
                break;
            }
            if latest.checkpoint.pending_nodes.is_empty() {
                let _ = state
                    .update_automation_v2_run(&run.run_id, |row| {
                        row.status = AutomationRunStatus::Completed;
                        row.detail = Some("automation run completed".to_string());
                    })
                    .await;
                break;
            }

            let completed = latest
                .checkpoint
                .completed_nodes
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            let pending = latest.checkpoint.pending_nodes.clone();
            let runnable = pending
                .iter()
                .filter_map(|node_id| {
                    let node = automation
                        .flow
                        .nodes
                        .iter()
                        .find(|n| n.node_id == *node_id)?;
                    if node.depends_on.iter().all(|dep| completed.contains(dep)) {
                        Some(node.clone())
                    } else {
                        None
                    }
                })
                .take(max_parallel)
                .collect::<Vec<_>>();

            if runnable.is_empty() {
                let _ = state
                    .update_automation_v2_run(&run.run_id, |row| {
                        row.status = AutomationRunStatus::Failed;
                        row.detail = Some("flow deadlock: no runnable nodes".to_string());
                    })
                    .await;
                break;
            }

            let runnable_node_ids = runnable
                .iter()
                .map(|node| node.node_id.clone())
                .collect::<Vec<_>>();
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    for node_id in &runnable_node_ids {
                        let attempts = row
                            .checkpoint
                            .node_attempts
                            .entry(node_id.clone())
                            .or_insert(0);
                        *attempts += 1;
                    }
                })
                .await;

            let tasks = runnable
                .iter()
                .map(|node| {
                    let Some(agent) = automation
                        .agents
                        .iter()
                        .find(|a| a.agent_id == node.agent_id)
                        .cloned()
                    else {
                        return futures::future::ready((
                            node.node_id.clone(),
                            Err(anyhow::anyhow!("agent not found")),
                        ))
                        .boxed();
                    };
                    let state = state.clone();
                    let run_id = run.run_id.clone();
                    let automation = automation.clone();
                    let node = node.clone();
                    async move {
                        let result =
                            execute_automation_v2_node(&state, &run_id, &automation, &node, &agent)
                                .await;
                        (node.node_id, result)
                    }
                    .boxed()
                })
                .collect::<Vec<_>>();
            let outcomes = join_all(tasks).await;

            let mut terminal_failure = None::<String>;
            let latest_attempts = state
                .get_automation_v2_run(&run.run_id)
                .await
                .map(|row| row.checkpoint.node_attempts)
                .unwrap_or_default();
            for (node_id, result) in outcomes {
                match result {
                    Ok(output) => {
                        let _ = state
                            .update_automation_v2_run(&run.run_id, |row| {
                                row.checkpoint.pending_nodes.retain(|id| id != &node_id);
                                if !row
                                    .checkpoint
                                    .completed_nodes
                                    .iter()
                                    .any(|id| id == &node_id)
                                {
                                    row.checkpoint.completed_nodes.push(node_id.clone());
                                }
                                row.checkpoint.node_outputs.insert(node_id.clone(), output);
                            })
                            .await;
                    }
                    Err(error) => {
                        let is_paused = state
                            .get_automation_v2_run(&run.run_id)
                            .await
                            .map(|row| row.status == AutomationRunStatus::Paused)
                            .unwrap_or(false);
                        if is_paused {
                            break;
                        }
                        let detail = truncate_text(&error.to_string(), 500);
                        let attempts = latest_attempts.get(&node_id).copied().unwrap_or(1);
                        let max_attempts = automation
                            .flow
                            .nodes
                            .iter()
                            .find(|row| row.node_id == node_id)
                            .map(automation_node_max_attempts)
                            .unwrap_or(1);
                        if attempts >= max_attempts {
                            terminal_failure = Some(format!(
                                "node `{}` failed after {}/{} attempts: {}",
                                node_id, attempts, max_attempts, detail
                            ));
                            break;
                        }
                        let _ = state
                            .update_automation_v2_run(&run.run_id, |row| {
                                row.detail = Some(format!(
                                    "retrying node `{}` after attempt {}/{} failed: {}",
                                    node_id, attempts, max_attempts, detail
                                ));
                            })
                            .await;
                    }
                }
            }
            if let Some(detail) = terminal_failure {
                let _ = state
                    .update_automation_v2_run(&run.run_id, |row| {
                        row.status = AutomationRunStatus::Failed;
                        row.detail = Some(detail);
                    })
                    .await;
                break;
            }
        }
    }
}

async fn build_routine_prompt(state: &AppState, run: &RoutineRunRecord) -> String {
    let normalized_entrypoint = run.entrypoint.trim();
    let known_tool = state
        .tools
        .list()
        .await
        .into_iter()
        .any(|schema| schema.name == normalized_entrypoint);
    if known_tool {
        let args = if run.args.is_object() {
            run.args.clone()
        } else {
            serde_json::json!({})
        };
        return format!("/tool {} {}", normalized_entrypoint, args);
    }

    if let Some(objective) = routine_objective_from_args(run) {
        return build_routine_mission_prompt(run, &objective);
    }

    format!(
        "Execute routine '{}' using entrypoint '{}' with args: {}",
        run.routine_id, run.entrypoint, run.args
    )
}

fn routine_objective_from_args(run: &RoutineRunRecord) -> Option<String> {
    run.args
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn routine_mode_from_args(args: &Value) -> &str {
    args.get("mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("standalone")
}

fn routine_success_criteria_from_args(args: &Value) -> Vec<String> {
    args.get("success_criteria")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str())
                .map(str::trim)
                .filter(|row| !row.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn build_routine_mission_prompt(run: &RoutineRunRecord, objective: &str) -> String {
    let mode = routine_mode_from_args(&run.args);
    let success_criteria = routine_success_criteria_from_args(&run.args);
    let orchestrator_only_tool_calls = run
        .args
        .get("orchestrator_only_tool_calls")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut lines = vec![
        format!("Automation ID: {}", run.routine_id),
        format!("Run ID: {}", run.run_id),
        format!("Mode: {}", mode),
        format!("Mission Objective: {}", objective),
    ];

    if !success_criteria.is_empty() {
        lines.push("Success Criteria:".to_string());
        for criterion in success_criteria {
            lines.push(format!("- {}", criterion));
        }
    }

    if run.allowed_tools.is_empty() {
        lines.push("Allowed Tools: all available by current policy".to_string());
    } else {
        lines.push(format!("Allowed Tools: {}", run.allowed_tools.join(", ")));
    }

    if run.output_targets.is_empty() {
        lines.push("Output Targets: none configured".to_string());
    } else {
        lines.push("Output Targets:".to_string());
        for target in &run.output_targets {
            lines.push(format!("- {}", target));
        }
    }

    if mode.eq_ignore_ascii_case("orchestrated") {
        lines.push("Execution Pattern: Plan -> Do -> Verify -> Notify".to_string());
        lines
            .push("Role Contract: Orchestrator owns final decisions and final output.".to_string());
        if orchestrator_only_tool_calls {
            lines.push(
                "Tool Policy: only the orchestrator may execute tools; helper roles propose actions/results."
                    .to_string(),
            );
        }
    } else {
        lines.push("Execution Pattern: Standalone mission run".to_string());
    }

    lines.push(
        "Deliverable: produce a concise final report that states what was done, what was verified, and final artifact locations."
            .to_string(),
    );

    lines.join("\n")
}

fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut out = input[..max_len].to_string();
    out.push_str("...<truncated>");
    out
}

async fn append_configured_output_artifacts(state: &AppState, run: &RoutineRunRecord) {
    if run.output_targets.is_empty() {
        return;
    }
    for target in &run.output_targets {
        let artifact = RoutineRunArtifact {
            artifact_id: format!("artifact-{}", uuid::Uuid::new_v4()),
            uri: target.clone(),
            kind: "output_target".to_string(),
            label: Some("configured output target".to_string()),
            created_at_ms: now_ms(),
            metadata: Some(serde_json::json!({
                "source": "routine.output_targets",
                "runID": run.run_id,
                "routineID": run.routine_id,
            })),
        };
        let _ = state
            .append_routine_run_artifact(&run.run_id, artifact.clone())
            .await;
        state.event_bus.publish(EngineEvent::new(
            "routine.run.artifact_added",
            serde_json::json!({
                "runID": run.run_id,
                "routineID": run.routine_id,
                "artifact": artifact,
            }),
        ));
    }
}

fn parse_model_spec(value: &Value) -> Option<ModelSpec> {
    let obj = value.as_object()?;
    let provider_id = obj.get("provider_id")?.as_str()?.trim();
    let model_id = obj.get("model_id")?.as_str()?.trim();
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some(ModelSpec {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

fn model_spec_for_role_from_args(args: &Value, role: &str) -> Option<ModelSpec> {
    args.get("model_policy")
        .and_then(|v| v.get("role_models"))
        .and_then(|v| v.get(role))
        .and_then(parse_model_spec)
}

fn default_model_spec_from_args(args: &Value) -> Option<ModelSpec> {
    args.get("model_policy")
        .and_then(|v| v.get("default_model"))
        .and_then(parse_model_spec)
}

fn default_model_spec_from_effective_config(config: &Value) -> Option<ModelSpec> {
    let provider_id = config
        .get("default_provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    let model_id = config
        .get("providers")
        .and_then(|v| v.get(provider_id))
        .and_then(|v| v.get("default_model"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    Some(ModelSpec {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

fn provider_catalog_has_model(providers: &[tandem_types::ProviderInfo], spec: &ModelSpec) -> bool {
    providers.iter().any(|provider| {
        provider.id == spec.provider_id
            && provider
                .models
                .iter()
                .any(|model| model.id == spec.model_id)
    })
}

async fn resolve_routine_model_spec_for_run(
    state: &AppState,
    run: &RoutineRunRecord,
) -> (Option<ModelSpec>, String) {
    let providers = state.providers.list().await;
    let mode = routine_mode_from_args(&run.args);
    let mut requested: Vec<(ModelSpec, &str)> = Vec::new();

    if mode.eq_ignore_ascii_case("orchestrated") {
        if let Some(orchestrator) = model_spec_for_role_from_args(&run.args, "orchestrator") {
            requested.push((orchestrator, "args.model_policy.role_models.orchestrator"));
        }
    }
    if let Some(default_model) = default_model_spec_from_args(&run.args) {
        requested.push((default_model, "args.model_policy.default_model"));
    }
    let effective_config = state.config.get_effective_value().await;
    if let Some(config_default) = default_model_spec_from_effective_config(&effective_config) {
        requested.push((config_default, "config.default_provider"));
    }

    for (candidate, source) in requested {
        if provider_catalog_has_model(&providers, &candidate) {
            return (Some(candidate), source.to_string());
        }
    }

    let fallback = providers
        .into_iter()
        .find(|provider| !provider.models.is_empty())
        .and_then(|provider| {
            let model = provider.models.first()?;
            Some(ModelSpec {
                provider_id: provider.id,
                model_id: model.id.clone(),
            })
        });

    (fallback, "provider_catalog_fallback".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state_with_path(path: PathBuf) -> AppState {
        let mut state = AppState::new_starting("test-attempt".to_string(), true);
        state.shared_resources_path = path;
        state.routines_path = tmp_routines_file("shared-state");
        state.routine_history_path = tmp_routines_file("routine-history");
        state.routine_runs_path = tmp_routines_file("routine-runs");
        state
    }

    fn tmp_resource_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tandem-server-{name}-{}.json",
            uuid::Uuid::new_v4()
        ))
    }

    fn tmp_routines_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tandem-server-routines-{name}-{}.json",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn default_model_spec_from_effective_config_reads_default_route() {
        let cfg = serde_json::json!({
            "default_provider": "openrouter",
            "providers": {
                "openrouter": {
                    "default_model": "google/gemini-3-flash-preview"
                }
            }
        });
        let spec = default_model_spec_from_effective_config(&cfg).expect("default model spec");
        assert_eq!(spec.provider_id, "openrouter");
        assert_eq!(spec.model_id, "google/gemini-3-flash-preview");
    }

    #[test]
    fn default_model_spec_from_effective_config_returns_none_when_incomplete() {
        let missing_provider = serde_json::json!({
            "providers": {
                "openrouter": {
                    "default_model": "google/gemini-3-flash-preview"
                }
            }
        });
        assert!(default_model_spec_from_effective_config(&missing_provider).is_none());

        let missing_model = serde_json::json!({
            "default_provider": "openrouter",
            "providers": {
                "openrouter": {}
            }
        });
        assert!(default_model_spec_from_effective_config(&missing_model).is_none());
    }

    #[tokio::test]
    async fn shared_resource_put_increments_revision() {
        let path = tmp_resource_file("shared-resource-put");
        let state = test_state_with_path(path.clone());

        let first = state
            .put_shared_resource(
                "project/demo/board".to_string(),
                serde_json::json!({"status":"todo"}),
                None,
                "agent-1".to_string(),
                None,
            )
            .await
            .expect("first put");
        assert_eq!(first.rev, 1);

        let second = state
            .put_shared_resource(
                "project/demo/board".to_string(),
                serde_json::json!({"status":"doing"}),
                Some(1),
                "agent-2".to_string(),
                Some(60_000),
            )
            .await
            .expect("second put");
        assert_eq!(second.rev, 2);
        assert_eq!(second.updated_by, "agent-2");
        assert_eq!(second.ttl_ms, Some(60_000));

        let raw = tokio::fs::read_to_string(path.clone())
            .await
            .expect("persisted");
        assert!(raw.contains("\"rev\": 2"));
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn shared_resource_put_detects_revision_conflict() {
        let path = tmp_resource_file("shared-resource-conflict");
        let state = test_state_with_path(path.clone());

        let _ = state
            .put_shared_resource(
                "mission/demo/card-1".to_string(),
                serde_json::json!({"title":"Card 1"}),
                None,
                "agent-1".to_string(),
                None,
            )
            .await
            .expect("seed put");

        let conflict = state
            .put_shared_resource(
                "mission/demo/card-1".to_string(),
                serde_json::json!({"title":"Card 1 edited"}),
                Some(99),
                "agent-2".to_string(),
                None,
            )
            .await
            .expect_err("expected conflict");

        match conflict {
            ResourceStoreError::RevisionConflict(conflict) => {
                assert_eq!(conflict.expected_rev, Some(99));
                assert_eq!(conflict.current_rev, Some(1));
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn shared_resource_rejects_invalid_namespace_key() {
        let path = tmp_resource_file("shared-resource-invalid-key");
        let state = test_state_with_path(path.clone());

        let error = state
            .put_shared_resource(
                "global/demo/key".to_string(),
                serde_json::json!({"x":1}),
                None,
                "agent-1".to_string(),
                None,
            )
            .await
            .expect_err("invalid key should fail");

        match error {
            ResourceStoreError::InvalidKey { key } => assert_eq!(key, "global/demo/key"),
            other => panic!("unexpected error: {other:?}"),
        }

        assert!(!path.exists());
    }

    #[test]
    fn derive_status_index_update_for_run_started() {
        let event = EngineEvent::new(
            "session.run.started",
            serde_json::json!({
                "sessionID": "s-1",
                "runID": "r-1"
            }),
        );
        let update = derive_status_index_update(&event).expect("update");
        assert_eq!(update.key, "run/s-1/status");
        assert_eq!(
            update.value.get("state").and_then(|v| v.as_str()),
            Some("running")
        );
        assert_eq!(
            update.value.get("phase").and_then(|v| v.as_str()),
            Some("run")
        );
    }

    #[test]
    fn derive_status_index_update_for_tool_invocation() {
        let event = EngineEvent::new(
            "message.part.updated",
            serde_json::json!({
                "sessionID": "s-2",
                "runID": "r-2",
                "part": { "type": "tool-invocation", "tool": "todo_write" }
            }),
        );
        let update = derive_status_index_update(&event).expect("update");
        assert_eq!(update.key, "run/s-2/status");
        assert_eq!(
            update.value.get("phase").and_then(|v| v.as_str()),
            Some("tool")
        );
        assert_eq!(
            update.value.get("toolActive").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            update.value.get("tool").and_then(|v| v.as_str()),
            Some("todo_write")
        );
    }

    #[test]
    fn misfire_skip_drops_runs_and_advances_next_fire() {
        let (count, next_fire) =
            compute_misfire_plan(10_500, 5_000, 1_000, &RoutineMisfirePolicy::Skip);
        assert_eq!(count, 0);
        assert_eq!(next_fire, 11_000);
    }

    #[test]
    fn misfire_run_once_emits_single_trigger() {
        let (count, next_fire) =
            compute_misfire_plan(10_500, 5_000, 1_000, &RoutineMisfirePolicy::RunOnce);
        assert_eq!(count, 1);
        assert_eq!(next_fire, 11_000);
    }

    #[test]
    fn misfire_catch_up_caps_trigger_count() {
        let (count, next_fire) = compute_misfire_plan(
            25_000,
            5_000,
            1_000,
            &RoutineMisfirePolicy::CatchUp { max_runs: 3 },
        );
        assert_eq!(count, 3);
        assert_eq!(next_fire, 26_000);
    }

    #[tokio::test]
    async fn routine_put_persists_and_loads() {
        let routines_path = tmp_routines_file("persist-load");
        let mut state = AppState::new_starting("routines-put".to_string(), true);
        state.routines_path = routines_path.clone();

        let routine = RoutineSpec {
            routine_id: "routine-1".to_string(),
            name: "Digest".to_string(),
            status: RoutineStatus::Active,
            schedule: RoutineSchedule::IntervalSeconds { seconds: 60 },
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
            entrypoint: "mission.default".to_string(),
            args: serde_json::json!({"topic":"status"}),
            allowed_tools: vec![],
            output_targets: vec![],
            creator_type: "user".to_string(),
            creator_id: "user-1".to_string(),
            requires_approval: true,
            external_integrations_allowed: false,
            next_fire_at_ms: Some(5_000),
            last_fired_at_ms: None,
        };

        state.put_routine(routine).await.expect("store routine");

        let mut reloaded = AppState::new_starting("routines-reload".to_string(), true);
        reloaded.routines_path = routines_path.clone();
        reloaded.load_routines().await.expect("load routines");
        let list = reloaded.list_routines().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].routine_id, "routine-1");

        let _ = tokio::fs::remove_file(routines_path).await;
    }

    #[tokio::test]
    async fn persist_routines_does_not_clobber_existing_store_with_empty_state() {
        let routines_path = tmp_routines_file("persist-guard");
        let mut writer = AppState::new_starting("routines-writer".to_string(), true);
        writer.routines_path = routines_path.clone();
        writer
            .put_routine(RoutineSpec {
                routine_id: "automation-guarded".to_string(),
                name: "Guarded Automation".to_string(),
                status: RoutineStatus::Active,
                schedule: RoutineSchedule::IntervalSeconds { seconds: 300 },
                timezone: "UTC".to_string(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
                entrypoint: "mission.default".to_string(),
                args: serde_json::json!({
                    "prompt": "Keep this saved across restart"
                }),
                allowed_tools: vec!["read".to_string()],
                output_targets: vec![],
                creator_type: "user".to_string(),
                creator_id: "user-1".to_string(),
                requires_approval: false,
                external_integrations_allowed: false,
                next_fire_at_ms: Some(5_000),
                last_fired_at_ms: None,
            })
            .await
            .expect("persist baseline routine");

        let mut empty_state = AppState::new_starting("routines-empty".to_string(), true);
        empty_state.routines_path = routines_path.clone();
        let persist = empty_state.persist_routines().await;
        assert!(
            persist.is_err(),
            "empty state should not overwrite existing routines store"
        );

        let raw = tokio::fs::read_to_string(&routines_path)
            .await
            .expect("read guarded routines file");
        let parsed: std::collections::HashMap<String, RoutineSpec> =
            serde_json::from_str(&raw).expect("parse guarded routines file");
        assert!(parsed.contains_key("automation-guarded"));

        let _ = tokio::fs::remove_file(routines_path.clone()).await;
        let _ = tokio::fs::remove_file(sibling_backup_path(&routines_path)).await;
    }

    #[tokio::test]
    async fn load_routines_recovers_from_backup_when_primary_corrupt() {
        let routines_path = tmp_routines_file("backup-recovery");
        let backup_path = sibling_backup_path(&routines_path);
        let mut state = AppState::new_starting("routines-backup-recovery".to_string(), true);
        state.routines_path = routines_path.clone();

        let primary = "{ not valid json";
        tokio::fs::write(&routines_path, primary)
            .await
            .expect("write corrupt primary");
        let backup = serde_json::json!({
            "routine-1": {
                "routine_id": "routine-1",
                "name": "Recovered",
                "status": "active",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "timezone": "UTC",
                "misfire_policy": { "type": "run_once" },
                "entrypoint": "mission.default",
                "args": {},
                "allowed_tools": [],
                "output_targets": [],
                "creator_type": "user",
                "creator_id": "u-1",
                "requires_approval": true,
                "external_integrations_allowed": false,
                "next_fire_at_ms": null,
                "last_fired_at_ms": null
            }
        });
        tokio::fs::write(&backup_path, serde_json::to_string_pretty(&backup).unwrap())
            .await
            .expect("write backup");

        state.load_routines().await.expect("load from backup");
        let list = state.list_routines().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].routine_id, "routine-1");

        let _ = tokio::fs::remove_file(routines_path).await;
        let _ = tokio::fs::remove_file(backup_path).await;
    }

    #[tokio::test]
    async fn evaluate_routine_misfires_respects_skip_run_once_and_catch_up() {
        let routines_path = tmp_routines_file("misfire-eval");
        let mut state = AppState::new_starting("routines-eval".to_string(), true);
        state.routines_path = routines_path.clone();

        let base = |id: &str, policy: RoutineMisfirePolicy| RoutineSpec {
            routine_id: id.to_string(),
            name: id.to_string(),
            status: RoutineStatus::Active,
            schedule: RoutineSchedule::IntervalSeconds { seconds: 1 },
            timezone: "UTC".to_string(),
            misfire_policy: policy,
            entrypoint: "mission.default".to_string(),
            args: serde_json::json!({}),
            allowed_tools: vec![],
            output_targets: vec![],
            creator_type: "user".to_string(),
            creator_id: "u-1".to_string(),
            requires_approval: false,
            external_integrations_allowed: false,
            next_fire_at_ms: Some(5_000),
            last_fired_at_ms: None,
        };

        state
            .put_routine(base("routine-skip", RoutineMisfirePolicy::Skip))
            .await
            .expect("put skip");
        state
            .put_routine(base("routine-once", RoutineMisfirePolicy::RunOnce))
            .await
            .expect("put once");
        state
            .put_routine(base(
                "routine-catch",
                RoutineMisfirePolicy::CatchUp { max_runs: 3 },
            ))
            .await
            .expect("put catch");

        let plans = state.evaluate_routine_misfires(10_500).await;
        let plan_skip = plans.iter().find(|p| p.routine_id == "routine-skip");
        let plan_once = plans.iter().find(|p| p.routine_id == "routine-once");
        let plan_catch = plans.iter().find(|p| p.routine_id == "routine-catch");

        assert!(plan_skip.is_none());
        assert_eq!(plan_once.map(|p| p.run_count), Some(1));
        assert_eq!(plan_catch.map(|p| p.run_count), Some(3));

        let stored = state.list_routines().await;
        let skip_next = stored
            .iter()
            .find(|r| r.routine_id == "routine-skip")
            .and_then(|r| r.next_fire_at_ms)
            .expect("skip next");
        assert!(skip_next > 10_500);

        let _ = tokio::fs::remove_file(routines_path).await;
    }

    #[test]
    fn routine_policy_blocks_external_side_effects_by_default() {
        let routine = RoutineSpec {
            routine_id: "routine-policy-1".to_string(),
            name: "Connector routine".to_string(),
            status: RoutineStatus::Active,
            schedule: RoutineSchedule::IntervalSeconds { seconds: 60 },
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
            entrypoint: "connector.email.reply".to_string(),
            args: serde_json::json!({}),
            allowed_tools: vec![],
            output_targets: vec![],
            creator_type: "user".to_string(),
            creator_id: "u-1".to_string(),
            requires_approval: true,
            external_integrations_allowed: false,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };

        let decision = evaluate_routine_execution_policy(&routine, "manual");
        assert!(matches!(decision, RoutineExecutionDecision::Blocked { .. }));
    }

    #[test]
    fn routine_policy_requires_approval_for_external_side_effects_when_enabled() {
        let routine = RoutineSpec {
            routine_id: "routine-policy-2".to_string(),
            name: "Connector routine".to_string(),
            status: RoutineStatus::Active,
            schedule: RoutineSchedule::IntervalSeconds { seconds: 60 },
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
            entrypoint: "connector.email.reply".to_string(),
            args: serde_json::json!({}),
            allowed_tools: vec![],
            output_targets: vec![],
            creator_type: "user".to_string(),
            creator_id: "u-1".to_string(),
            requires_approval: true,
            external_integrations_allowed: true,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };

        let decision = evaluate_routine_execution_policy(&routine, "manual");
        assert!(matches!(
            decision,
            RoutineExecutionDecision::RequiresApproval { .. }
        ));
    }

    #[test]
    fn routine_policy_allows_non_external_entrypoints() {
        let routine = RoutineSpec {
            routine_id: "routine-policy-3".to_string(),
            name: "Internal mission routine".to_string(),
            status: RoutineStatus::Active,
            schedule: RoutineSchedule::IntervalSeconds { seconds: 60 },
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
            entrypoint: "mission.default".to_string(),
            args: serde_json::json!({}),
            allowed_tools: vec![],
            output_targets: vec![],
            creator_type: "user".to_string(),
            creator_id: "u-1".to_string(),
            requires_approval: true,
            external_integrations_allowed: false,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };

        let decision = evaluate_routine_execution_policy(&routine, "manual");
        assert_eq!(decision, RoutineExecutionDecision::Allowed);
    }

    #[tokio::test]
    async fn claim_next_queued_routine_run_marks_oldest_running() {
        let mut state = AppState::new_starting("routine-claim".to_string(), true);
        state.routine_runs_path = tmp_routines_file("routine-claim-runs");

        let mk = |run_id: &str, created_at_ms: u64| RoutineRunRecord {
            run_id: run_id.to_string(),
            routine_id: "routine-claim".to_string(),
            trigger_type: "manual".to_string(),
            run_count: 1,
            status: RoutineRunStatus::Queued,
            created_at_ms,
            updated_at_ms: created_at_ms,
            fired_at_ms: Some(created_at_ms),
            started_at_ms: None,
            finished_at_ms: None,
            requires_approval: false,
            approval_reason: None,
            denial_reason: None,
            paused_reason: None,
            detail: None,
            entrypoint: "mission.default".to_string(),
            args: serde_json::json!({}),
            allowed_tools: vec![],
            output_targets: vec![],
            artifacts: vec![],
            active_session_ids: vec![],
            latest_session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        };

        {
            let mut guard = state.routine_runs.write().await;
            guard.insert("run-late".to_string(), mk("run-late", 2_000));
            guard.insert("run-early".to_string(), mk("run-early", 1_000));
        }
        state.persist_routine_runs().await.expect("persist");

        let claimed = state
            .claim_next_queued_routine_run()
            .await
            .expect("claimed run");
        assert_eq!(claimed.run_id, "run-early");
        assert_eq!(claimed.status, RoutineRunStatus::Running);
        assert!(claimed.started_at_ms.is_some());
    }

    #[tokio::test]
    async fn routine_session_policy_roundtrip_normalizes_tools() {
        let state = AppState::new_starting("routine-policy-hook".to_string(), true);
        state
            .set_routine_session_policy(
                "session-routine-1".to_string(),
                "run-1".to_string(),
                "routine-1".to_string(),
                vec![
                    "read".to_string(),
                    " mcp.arcade.search ".to_string(),
                    "read".to_string(),
                    "".to_string(),
                ],
            )
            .await;

        let policy = state
            .routine_session_policy("session-routine-1")
            .await
            .expect("policy");
        assert_eq!(
            policy.allowed_tools,
            vec!["read".to_string(), "mcp.arcade.search".to_string()]
        );
    }

    #[tokio::test]
    async fn routine_run_preserves_latest_session_id_after_session_clears() {
        let state = AppState::new_starting("routine-latest-session".to_string(), true);
        let routine = RoutineSpec {
            routine_id: "routine-session-link".to_string(),
            name: "Routine Session Link".to_string(),
            status: RoutineStatus::Active,
            schedule: RoutineSchedule::IntervalSeconds { seconds: 300 },
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::Skip,
            entrypoint: "mission.default".to_string(),
            args: serde_json::json!({}),
            allowed_tools: vec![],
            output_targets: vec![],
            creator_type: "user".to_string(),
            creator_id: "test".to_string(),
            requires_approval: false,
            external_integrations_allowed: false,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };

        let run = state
            .create_routine_run(&routine, "manual", 1, RoutineRunStatus::Queued, None)
            .await;
        state
            .add_active_session_id(&run.run_id, "session-123".to_string())
            .await
            .expect("active session added");
        state
            .clear_active_session_id(&run.run_id, "session-123")
            .await
            .expect("active session cleared");

        let updated = state
            .get_routine_run(&run.run_id)
            .await
            .expect("run exists");
        assert!(updated.active_session_ids.is_empty());
        assert_eq!(updated.latest_session_id.as_deref(), Some("session-123"));
    }

    #[test]
    fn routine_mission_prompt_includes_orchestrated_contract() {
        let run = RoutineRunRecord {
            run_id: "run-orchestrated-1".to_string(),
            routine_id: "automation-orchestrated".to_string(),
            trigger_type: "manual".to_string(),
            run_count: 1,
            status: RoutineRunStatus::Queued,
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            fired_at_ms: Some(1_000),
            started_at_ms: None,
            finished_at_ms: None,
            requires_approval: true,
            approval_reason: None,
            denial_reason: None,
            paused_reason: None,
            detail: None,
            entrypoint: "mission.default".to_string(),
            args: serde_json::json!({
                "prompt": "Coordinate a multi-step release readiness check.",
                "mode": "orchestrated",
                "success_criteria": ["All blockers listed", "Output artifact written"],
                "orchestrator_only_tool_calls": true
            }),
            allowed_tools: vec!["read".to_string(), "webfetch".to_string()],
            output_targets: vec!["file://reports/release-readiness.md".to_string()],
            artifacts: vec![],
            active_session_ids: vec![],
            latest_session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        };

        let objective = routine_objective_from_args(&run).expect("objective");
        let prompt = build_routine_mission_prompt(&run, &objective);

        assert!(prompt.contains("Mode: orchestrated"));
        assert!(prompt.contains("Plan -> Do -> Verify -> Notify"));
        assert!(prompt.contains("only the orchestrator may execute tools"));
        assert!(prompt.contains("Allowed Tools: read, webfetch"));
        assert!(prompt.contains("file://reports/release-readiness.md"));
    }

    #[test]
    fn routine_mission_prompt_includes_standalone_defaults() {
        let run = RoutineRunRecord {
            run_id: "run-standalone-1".to_string(),
            routine_id: "automation-standalone".to_string(),
            trigger_type: "manual".to_string(),
            run_count: 1,
            status: RoutineRunStatus::Queued,
            created_at_ms: 2_000,
            updated_at_ms: 2_000,
            fired_at_ms: Some(2_000),
            started_at_ms: None,
            finished_at_ms: None,
            requires_approval: false,
            approval_reason: None,
            denial_reason: None,
            paused_reason: None,
            detail: None,
            entrypoint: "mission.default".to_string(),
            args: serde_json::json!({
                "prompt": "Summarize top engineering updates.",
                "success_criteria": ["Three bullet summary"]
            }),
            allowed_tools: vec![],
            output_targets: vec![],
            artifacts: vec![],
            active_session_ids: vec![],
            latest_session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        };

        let objective = routine_objective_from_args(&run).expect("objective");
        let prompt = build_routine_mission_prompt(&run, &objective);

        assert!(prompt.contains("Mode: standalone"));
        assert!(prompt.contains("Execution Pattern: Standalone mission run"));
        assert!(prompt.contains("Allowed Tools: all available by current policy"));
        assert!(prompt.contains("Output Targets: none configured"));
    }

    #[test]
    fn shared_resource_key_validator_accepts_swarm_active_tasks() {
        assert!(is_valid_resource_key("swarm.active_tasks"));
        assert!(is_valid_resource_key("project/demo"));
        assert!(!is_valid_resource_key("swarm//active_tasks"));
        assert!(!is_valid_resource_key("misc/demo"));
    }
}
