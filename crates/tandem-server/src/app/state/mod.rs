use crate::config::channels::normalize_allowed_tools;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::Context;
use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_enterprise_contract::{
    authority::{
        AuthorityAccessRequest, AuthorityDecision, AuthorityEffect, IntraTenantAuthorityGraph,
    },
    governance::GovernancePolicyEngine,
    ConnectorInstance as EnterpriseConnectorInstance,
    CrossTenantGrantRecord as EnterpriseCrossTenantGrantRecord,
    IngestionJob as EnterpriseIngestionJob, IngestionQuarantine as EnterpriseIngestionQuarantine,
    OrganizationUnit as EnterpriseOrganizationUnit,
    OrganizationUnitAccessGrant as EnterpriseOrganizationUnitAccessGrant,
    OrganizationUnitMembership as EnterpriseOrganizationUnitMembership, ScopedGrant,
    SourceBinding as EnterpriseSourceBinding,
};
use tandem_memory::types::{MemorySourceAccessTarget, MemoryTier};
use tandem_orchestrator::MissionState;
use tandem_tools::{
    ToolDispatchContext, ToolDispatchLedger, ToolDispatchLedgerEvent, ToolDispatchSource,
};
use tandem_types::{
    ApprovalGateMatrix, EngineEvent, GateOutcome, GateRequest, HostRuntimeContext, MessagePart,
    ModelSpec, PolicyDecisionEffect, PolicyDecisionRecord, TenantContext,
};
use tokio::fs;
use tokio::sync::RwLock;

use tandem_channels::{
    channel_registry::registered_channels,
    config::{ChannelsConfig, DiscordConfig, SlackConfig, TelegramConfig},
};
use tandem_core::{
    resolve_shared_paths, PromptContextHook, PromptContextHookContext, PromptContextHookResult,
    PromptContextHookStats,
};
use tandem_memory::db::MemoryDatabase;
use tandem_providers::ChatMessage;
use tandem_workflows::{
    load_registry as load_workflow_registry, validate_registry as validate_workflow_registry,
    WorkflowHookBinding, WorkflowLoadSource, WorkflowRegistry, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowSourceKind, WorkflowSpec, WorkflowValidationMessage,
};

use crate::agent_teams::AgentTeamRuntime;
use crate::app::rate_limit::ChannelRateLimiter;
use crate::app::startup::{StartupSnapshot, StartupState, StartupStatus};
use crate::automation_v2::governance::GovernanceState;
use crate::automation_v2::types::*;
use crate::bug_monitor::types::*;
use crate::capability_resolver::CapabilityResolver;
use crate::config::{self, channels::ChannelsConfigFile, webui::WebUiConfig};
use crate::goal_capability_learning::GoalCapabilityLearningDecisionStore;
use crate::memory::types::{GovernedMemoryRecord, MemoryAuditEvent};
use crate::pack_manager::PackManager;
use crate::preset_registry::PresetRegistry;
use crate::routines::{errors::RoutineStoreError, types::*};
use crate::runtime::{
    lease::EngineLease, runs::RunRegistry, state::RuntimeState, worktrees::ManagedWorktreeRecord,
};
use crate::shared_resources::types::{ResourceConflict, ResourceStoreError, SharedResourceRecord};
use crate::util::{host::detect_host_runtime_context, time::now_ms};
use crate::{
    derive_phase1_metrics_from_run, derive_phase1_validator_case_outcomes_from_run,
    establish_phase1_baseline, evaluate_phase1_promotion, optimization_snapshot_hash,
    parse_phase1_metrics, phase1_baseline_replay_due, validate_phase1_candidate_mutation,
    OptimizationBaselineReplayRecord, OptimizationCampaignRecord, OptimizationCampaignStatus,
    OptimizationExperimentRecord, OptimizationExperimentStatus, OptimizationMutableField,
    OptimizationPromotionDecisionKind,
};

pub mod approval_message_map;
mod automation_v2_run_store;
pub mod channel_user_capabilities;
pub mod enterprise_state;
mod oauth_state;
mod prompt_context_blocks;
mod prompt_context_hook;
mod prompt_memory_context;

pub(crate) use automation_v2_run_store::*;
pub use enterprise_state::EnterpriseState;
pub use oauth_state::OAuthState;
use prompt_context_hook::*;

#[derive(Clone)]
pub struct AppState {
    pub runtime: Arc<OnceLock<RuntimeState>>,
    pub startup: Arc<RwLock<StartupState>>,
    pub in_process_mode: Arc<AtomicBool>,
    pub api_token: Arc<RwLock<Option<String>>>,
    pub engine_leases: Arc<RwLock<std::collections::HashMap<String, EngineLease>>>,
    pub managed_worktrees: Arc<RwLock<std::collections::HashMap<String, ManagedWorktreeRecord>>>,
    pub run_registry: RunRegistry,
    pub run_stale_ms: u64,
    pub memory_records: Arc<RwLock<std::collections::HashMap<String, GovernedMemoryRecord>>>,
    pub memory_audit_log: Arc<RwLock<Vec<MemoryAuditEvent>>>,
    pub memory_db_path: PathBuf,
    pub memory_audit_path: PathBuf,
    pub protected_audit_path: PathBuf,
    /// EAA-12 (TAN-37): enterprise registries + persistence paths, owned as one
    /// wrapper instead of scattered across `AppState` (see [`EnterpriseState`]).
    pub enterprise: EnterpriseState,
    pub missions: Arc<RwLock<std::collections::HashMap<String, MissionState>>>,
    pub shared_resources: Arc<RwLock<std::collections::HashMap<String, SharedResourceRecord>>>,
    pub shared_resources_path: PathBuf,
    pub routines: Arc<RwLock<std::collections::HashMap<String, RoutineSpec>>>,
    pub routine_history: Arc<RwLock<std::collections::HashMap<String, Vec<RoutineHistoryEvent>>>>,
    pub routine_runs: Arc<RwLock<std::collections::HashMap<String, RoutineRunRecord>>>,
    pub automations_v2: Arc<RwLock<std::collections::HashMap<String, AutomationV2Spec>>>,
    pub channel_automation_drafts: Arc<
        RwLock<
            std::collections::HashMap<
                String,
                crate::http::channel_automation_drafts::ChannelAutomationDraftRecord,
            >,
        >,
    >,
    pub channel_user_capabilities: Arc<
        RwLock<
            std::collections::HashMap<
                String,
                channel_user_capabilities::ChannelUserCapabilityRecord,
            >,
        >,
    >,
    pub channel_enrollment_codes: Arc<
        RwLock<
            std::collections::HashMap<
                String,
                channel_user_capabilities::ChannelEnrollmentCodeRecord,
            >,
        >,
    >,
    /// GOV-B5b: per-identity, expiring channel step-up grants (key
    /// `"{channel}:{user_id}"` -> `expires_at_ms`). Ephemeral / in-memory: a
    /// step-up is an out-of-band assurance issued by the control panel and is
    /// short-lived by design, so it is intentionally not persisted.
    pub channel_step_up_grants: Arc<RwLock<std::collections::HashMap<String, u64>>>,
    pub memory_retrieval_budget_windows:
        Arc<RwLock<std::collections::HashMap<String, tandem_memory::MemoryRetrievalBudgetWindow>>>,
    pub channel_rate_limiter: Arc<ChannelRateLimiter>,
    pub automation_governance: Arc<RwLock<GovernanceState>>,
    pub governance_engine: Arc<dyn GovernancePolicyEngine>,
    pub automation_v2_runs: Arc<RwLock<std::collections::HashMap<String, AutomationV2RunRecord>>>,
    pub automation_scheduler: Arc<RwLock<automation::AutomationScheduler>>,
    pub automation_scheduler_stopping: Arc<AtomicBool>,
    pub automations_v2_persistence: Arc<tokio::sync::Mutex<()>>,
    pub workflow_plans: Arc<RwLock<std::collections::HashMap<String, WorkflowPlan>>>,
    pub workflow_plan_drafts:
        Arc<RwLock<std::collections::HashMap<String, WorkflowPlanDraftRecord>>>,
    pub workflow_planner_sessions: Arc<
        RwLock<
            std::collections::HashMap<
                String,
                crate::http::workflow_planner::WorkflowPlannerSessionRecord,
            >,
        >,
    >,
    pub workflow_learning_candidates:
        Arc<RwLock<std::collections::HashMap<String, WorkflowLearningCandidate>>>,
    pub(crate) context_packs: Arc<
        RwLock<std::collections::HashMap<String, crate::http::context_packs::ContextPackRecord>>,
    >,
    pub optimization_campaigns:
        Arc<RwLock<std::collections::HashMap<String, OptimizationCampaignRecord>>>,
    pub optimization_experiments:
        Arc<RwLock<std::collections::HashMap<String, OptimizationExperimentRecord>>>,
    pub bug_monitor_config: Arc<RwLock<BugMonitorConfig>>,
    pub bug_monitor_drafts: Arc<RwLock<std::collections::HashMap<String, BugMonitorDraftRecord>>>,
    pub bug_monitor_incidents:
        Arc<RwLock<std::collections::HashMap<String, BugMonitorIncidentRecord>>>,
    pub bug_monitor_posts: Arc<RwLock<std::collections::HashMap<String, BugMonitorPostRecord>>>,
    pub bug_monitor_log_watcher_state_path: PathBuf,
    pub bug_monitor_log_source_states:
        Arc<RwLock<std::collections::HashMap<String, BugMonitorLogSourceState>>>,
    pub bug_monitor_log_watcher_status: Arc<RwLock<BugMonitorLogWatcherStatus>>,
    pub bug_monitor_log_evidence_dir: PathBuf,
    pub bug_monitor_intake_keys:
        Arc<RwLock<std::collections::HashMap<String, BugMonitorProjectIntakeKey>>>,
    pub bug_monitor_intake_keys_path: PathBuf,
    pub external_actions: Arc<RwLock<std::collections::HashMap<String, ExternalActionRecord>>>,
    pub policy_decisions: Arc<RwLock<std::collections::HashMap<String, PolicyDecisionRecord>>>,
    pub goal_capability_learning_store: Arc<GoalCapabilityLearningDecisionStore>,
    pub bug_monitor_runtime_status: Arc<RwLock<BugMonitorRuntimeStatus>>,
    pub(crate) oauth: OAuthState,
    pub workflows: Arc<RwLock<WorkflowRegistry>>,
    pub workflow_runs: Arc<RwLock<std::collections::HashMap<String, WorkflowRunRecord>>>,
    pub workflow_hook_overrides: Arc<RwLock<std::collections::HashMap<String, bool>>>,
    pub workflow_dispatch_seen: Arc<RwLock<std::collections::HashMap<String, u64>>>,
    pub routine_session_policies:
        Arc<RwLock<std::collections::HashMap<String, RoutineSessionPolicy>>>,
    pub automation_v2_session_runs: Arc<RwLock<std::collections::HashMap<String, String>>>,
    pub automation_v2_session_mcp_servers:
        Arc<RwLock<std::collections::HashMap<String, Vec<String>>>>,
    pub token_cost_per_1k_usd: f64,
    pub routines_path: PathBuf,
    pub routine_history_path: PathBuf,
    pub routine_runs_path: PathBuf,
    pub automations_v2_path: PathBuf,
    pub channel_automation_drafts_path: PathBuf,
    pub channel_user_capabilities_path: PathBuf,
    pub automation_governance_path: PathBuf,
    pub automation_v2_runs_path: PathBuf,
    pub automation_v2_runs_archive_path: PathBuf,
    pub runtime_events_path: PathBuf,
    pub optimization_campaigns_path: PathBuf,
    pub optimization_experiments_path: PathBuf,
    pub bug_monitor_config_path: PathBuf,
    pub bug_monitor_drafts_path: PathBuf,
    pub bug_monitor_incidents_path: PathBuf,
    pub bug_monitor_posts_path: PathBuf,
    pub external_actions_path: PathBuf,
    pub policy_decisions_path: PathBuf,
    pub workflow_runs_path: PathBuf,
    pub workflow_planner_sessions_path: PathBuf,
    pub workflow_learning_candidates_path: PathBuf,
    pub context_packs_path: PathBuf,
    pub workflow_hook_overrides_path: PathBuf,
    pub agent_teams: AgentTeamRuntime,
    pub web_ui_enabled: Arc<AtomicBool>,
    pub web_ui_prefix: Arc<std::sync::RwLock<String>>,
    pub server_base_url: Arc<std::sync::RwLock<String>>,
    /// Test-only opt-in used by `test_support::test_state`; feature flags must
    /// not make runtime local-mode tenant headers trusted by default.
    pub(crate) trust_test_tenant_headers: Arc<AtomicBool>,
    pub channels_runtime: Arc<tokio::sync::Mutex<ChannelRuntime>>,
    pub host_runtime_context: HostRuntimeContext,
    pub pack_manager: Arc<PackManager>,
    pub capability_resolver: Arc<CapabilityResolver>,
    pub preset_registry: Arc<PresetRegistry>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelStatus {
    pub enabled: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub active_sessions: u64,
    pub meta: Value,
}

#[derive(Clone)]
struct AppStateToolDispatchLedger {
    event_bus: tandem_core::EventBus,
}

#[async_trait::async_trait]
impl ToolDispatchLedger for AppStateToolDispatchLedger {
    async fn record(&self, event: ToolDispatchLedgerEvent) {
        self.event_bus.publish(EngineEvent::new(
            "tool.dispatch.recorded",
            serde_json::to_value(event).unwrap_or(Value::Null),
        ));
    }
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

pub struct ChannelRuntime {
    pub listeners: Option<tokio::task::JoinSet<()>>,
    pub statuses: std::collections::HashMap<String, ChannelStatus>,
    pub diagnostics: tandem_channels::channel_registry::ChannelRuntimeDiagnostics,
}

impl Default for ChannelRuntime {
    fn default() -> Self {
        Self {
            listeners: None,
            statuses: std::collections::HashMap::new(),
            diagnostics: tandem_channels::new_channel_runtime_diagnostics(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatusIndexUpdate {
    pub key: String,
    pub value: Value,
}

include!("app_state_impl_parts/part01.rs");
include!("app_state_impl_parts/part02.rs");
include!("app_state_impl_parts/part10.rs");
include!("app_state_impl_parts/part03.rs");
include!("app_state_impl_parts/part05.rs");
include!("app_state_impl_parts/part04.rs");
pub(crate) mod governance;

/// Returns the canonical filename for a handoff artifact JSON file.
fn handoff_filename(handoff_id: &str) -> String {
    // Sanitize the ID so it's safe as a filename component.
    let safe: String = handoff_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}.json")
}

/// Scan the `approved_dir` for a handoff that targets `target_automation_id` and
/// optionally matches `source_automation_id` and `artifact_type` filters.
/// Returns the first matching handoff (oldest by `created_at_ms`), or `None`.
///
/// Bounds: skips the scan entirely if the directory doesn't exist; caps the scan
/// at 256 entries to prevent scheduler stall on large directories.
async fn find_matching_handoff(
    approved_dir: &std::path::Path,
    target_automation_id: &str,
    source_filter: Option<&str>,
    artifact_type_filter: Option<&str>,
) -> Option<crate::automation_v2::types::HandoffArtifact> {
    use tokio::fs;
    if !approved_dir.exists() {
        return None;
    }

    let mut entries = match fs::read_dir(approved_dir).await {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!("handoff watch: failed to read approved dir: {err}");
            return None;
        }
    };

    let mut candidates: Vec<crate::automation_v2::types::HandoffArtifact> = Vec::new();
    let mut scanned = 0usize;

    while let Ok(Some(entry)) = entries.next_entry().await {
        if scanned >= 256 {
            break;
        }
        scanned += 1;

        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let raw = match fs::read_to_string(&path).await {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let handoff: crate::automation_v2::types::HandoffArtifact = match serde_json::from_str(&raw)
        {
            Ok(h) => h,
            Err(_) => continue,
        };

        // Check target match (always required).
        if handoff.target_automation_id != target_automation_id {
            continue;
        }
        // Optional source filter.
        if let Some(src) = source_filter {
            if handoff.source_automation_id != src {
                continue;
            }
        }
        // Optional artifact type filter.
        if let Some(kind) = artifact_type_filter {
            if handoff.artifact_type != kind {
                continue;
            }
        }
        // Skip already-consumed handoffs (shouldn't be in approved/ but be defensive).
        if handoff.consumed_by_run_id.is_some() {
            continue;
        }
        candidates.push(handoff);
    }

    // Return the oldest unmatched handoff so we process them in arrival order.
    candidates.into_iter().min_by_key(|h| h.created_at_ms)
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
            allowed_users: config::channels::normalize_allowed_users_or_wildcard(cfg.allowed_users),
            mention_only: cfg.mention_only,
            style_profile: cfg.style_profile,
            security_profile: cfg.security_profile,
        }),
        discord: channels.discord.clone().map(|cfg| DiscordConfig {
            bot_token: cfg.bot_token,
            guild_id: cfg.guild_id.and_then(|value| {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            }),
            allowed_users: config::channels::normalize_allowed_users_or_wildcard(cfg.allowed_users),
            mention_only: cfg.mention_only,
            security_profile: cfg.security_profile,
        }),
        slack: channels.slack.clone().map(|cfg| SlackConfig {
            bot_token: cfg.bot_token,
            channel_id: cfg.channel_id,
            allowed_users: config::channels::normalize_allowed_users_or_wildcard(cfg.allowed_users),
            mention_only: cfg.mention_only,
            security_profile: cfg.security_profile,
        }),
        server_base_url: state.server_base_url(),
        api_token: state.api_token().await.unwrap_or_default(),
        context_assertion: std::env::var("TANDEM_CHANNEL_CONTEXT_ASSERTION")
            .ok()
            .or_else(|| std::env::var("TANDEM_CONTEXT_ASSERTION").ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        tool_policy: channels.tool_policy.clone(),
    })
}

// channel config normalization moved to crate::config::channels

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

fn legacy_automations_v2_path() -> Option<PathBuf> {
    config::paths::resolve_legacy_root_file_path("automations_v2.json")
        .filter(|path| path != &config::paths::resolve_automations_v2_path())
}

fn candidate_automations_v2_paths(active_path: &PathBuf) -> Vec<PathBuf> {
    let mut candidates = vec![active_path.clone()];
    if let Some(legacy_path) = legacy_automations_v2_path() {
        if !candidates.contains(&legacy_path) {
            candidates.push(legacy_path);
        }
    }
    let default_path = config::paths::default_state_dir().join("automations_v2.json");
    if !candidates.contains(&default_path) {
        candidates.push(default_path);
    }
    candidates
}

async fn cleanup_stale_legacy_automations_v2_file(active_path: &PathBuf) -> anyhow::Result<()> {
    let Some(legacy_path) = legacy_automations_v2_path() else {
        return Ok(());
    };
    if legacy_path == *active_path || !legacy_path.exists() {
        return Ok(());
    }
    fs::remove_file(&legacy_path).await?;
    tracing::info!(
        active_path = active_path.display().to_string(),
        removed_path = legacy_path.display().to_string(),
        "removed stale legacy automation v2 file after canonical persistence"
    );
    Ok(())
}

async fn cleanup_stale_legacy_automation_v2_runs_file(active_path: &PathBuf) -> anyhow::Result<()> {
    let Some(legacy_path) = legacy_automation_v2_runs_path() else {
        return Ok(());
    };
    if legacy_path == *active_path || !legacy_path.exists() {
        return Ok(());
    }
    fs::remove_file(&legacy_path).await?;
    tracing::info!(
        active_path = active_path.display().to_string(),
        removed_path = legacy_path.display().to_string(),
        "removed stale legacy automation v2 runs file after canonical persistence"
    );
    Ok(())
}

fn legacy_automation_v2_runs_path() -> Option<PathBuf> {
    config::paths::resolve_legacy_root_file_path("automation_v2_runs.json")
        .filter(|path| path != &config::paths::resolve_automation_v2_runs_path())
}

fn candidate_automation_v2_runs_paths(active_path: &PathBuf) -> Vec<PathBuf> {
    let mut candidates = vec![active_path.clone()];
    if let Some(legacy_path) = legacy_automation_v2_runs_path() {
        if !candidates.contains(&legacy_path) {
            candidates.push(legacy_path);
        }
    }
    let default_path = config::paths::default_state_dir().join("automation_v2_runs.json");
    if !candidates.contains(&default_path) {
        candidates.push(default_path);
    }
    candidates
}

fn parse_automation_v2_file(raw: &str) -> std::collections::HashMap<String, AutomationV2Spec> {
    if raw.trim().is_empty() {
        return std::collections::HashMap::new();
    }
    serde_json::from_str::<std::collections::HashMap<String, AutomationV2Spec>>(raw)
        .unwrap_or_default()
}

fn parse_automation_v2_file_strict(
    raw: &str,
) -> anyhow::Result<std::collections::HashMap<String, AutomationV2Spec>> {
    if raw.trim().is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    serde_json::from_str::<std::collections::HashMap<String, AutomationV2Spec>>(raw)
        .map_err(anyhow::Error::from)
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AutomationV2DefinitionIndexFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    definitions: Vec<AutomationV2DefinitionIndexEntry>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AutomationV2DefinitionIndexEntry {
    automation_id: String,
    path: String,
    updated_at_ms: u64,
}

fn automation_v2_definitions_root(active_path: &Path) -> PathBuf {
    active_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("automations-v2")
}

/// Sanitize an ID to prevent path traversal attacks.
/// Converts any character outside the safe set to underscore.
/// Safe characters: alphanumeric, hyphen, underscore (commonly used in IDs).
fn sanitize_path_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Validate that a path is within the expected base directory.
/// Prevents path traversal attacks like "../../../etc/passwd".
fn validate_path_within_root(base: &Path, target: &Path) -> anyhow::Result<()> {
    let canonical_base = base
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("failed to canonicalize base path: {e}"))?;
    let canonical_target = target
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("failed to canonicalize target path: {e}"))?;

    if !canonical_target.starts_with(&canonical_base) {
        anyhow::bail!(
            "path traversal attempt: {:?} is not within base directory {:?}",
            canonical_target,
            canonical_base
        );
    }
    Ok(())
}

fn automation_v2_definition_file_name(automation_id: &str) -> String {
    let sanitized = sanitize_path_id(automation_id);
    let stem = if sanitized.trim().is_empty() {
        "automation".to_string()
    } else {
        sanitized
    };
    format!("{stem}.json")
}

fn automation_v2_definition_shard_path(active_path: &Path, automation_id: &str) -> PathBuf {
    automation_v2_definitions_root(active_path)
        .join(automation_v2_definition_file_name(automation_id))
}

fn automation_v2_definitions_index_path(active_path: &Path) -> PathBuf {
    automation_v2_definitions_root(active_path).join("index.json")
}

async fn load_automation_v2_definition_shards(
    active_path: &Path,
) -> anyhow::Result<std::collections::HashMap<String, AutomationV2Spec>> {
    let root = automation_v2_definitions_root(active_path);
    if !root.exists() {
        return Ok(std::collections::HashMap::new());
    }
    let mut out = std::collections::HashMap::new();
    let mut entries = fs::read_dir(&root).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file()
            || path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name == "index.json")
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        let raw = fs::read_to_string(&path).await?;
        let automation = serde_json::from_str::<AutomationV2Spec>(&raw).map_err(|error| {
            anyhow::anyhow!(
                "failed to parse automation v2 definition shard `{}`: {}",
                path.display(),
                error
            )
        })?;
        out.insert(automation.automation_id.clone(), automation);
    }
    Ok(out)
}

async fn persist_automation_v2_definition_shards(
    active_path: &Path,
    automations: &std::collections::HashMap<String, AutomationV2Spec>,
) -> anyhow::Result<()> {
    let root = automation_v2_definitions_root(active_path);
    fs::create_dir_all(&root).await?;
    let mut expected_paths = std::collections::HashSet::new();
    let mut index = AutomationV2DefinitionIndexFile {
        schema_version: 1,
        definitions: Vec::new(),
    };
    for automation in automations.values() {
        let path = automation_v2_definition_shard_path(active_path, &automation.automation_id);
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("automation.json")
            .to_string();
        expected_paths.insert(path.clone());
        index.definitions.push(AutomationV2DefinitionIndexEntry {
            automation_id: automation.automation_id.clone(),
            path: file_name,
            updated_at_ms: automation.updated_at_ms,
        });
        let payload = serde_json::to_string_pretty(automation)?;
        write_string_atomic(&path, &payload).await?;
    }
    index
        .definitions
        .sort_by(|a, b| a.automation_id.cmp(&b.automation_id));
    let index_payload = serde_json::to_string_pretty(&index)?;
    write_string_atomic(
        &automation_v2_definitions_index_path(active_path),
        &index_payload,
    )
    .await?;

    let mut entries = fs::read_dir(&root).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file()
            || path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name == "index.json")
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        if !expected_paths.contains(&path) {
            fs::remove_file(path).await?;
        }
    }
    Ok(())
}

async fn archive_automation_v2_aggregate_file(active_path: &Path) -> anyhow::Result<()> {
    if !active_path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(active_path).await?;
    if parse_automation_v2_file(&raw).is_empty() {
        fs::remove_file(active_path).await?;
        return Ok(());
    }
    let active_stem = active_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("automations_v2");
    let archive_path = active_path.with_file_name(format!("{active_stem}.legacy-aggregate.json"));
    if archive_path.exists() {
        fs::remove_file(active_path).await?;
        return Ok(());
    }
    fs::rename(active_path, &archive_path).await?;
    tracing::info!(
        active_path = active_path.display().to_string(),
        archive_path = archive_path.display().to_string(),
        "archived legacy automation v2 aggregate file after shard persistence"
    );
    Ok(())
}

async fn write_string_atomic(path: &Path, payload: &str) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("state.json");
    let temp_path = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::write(&temp_path, payload).await?;
    if let Err(error) = fs::rename(&temp_path, path).await {
        let _ = fs::remove_file(&temp_path).await;
        return Err(error.into());
    }
    Ok(())
}

async fn read_state_file_with_legacy(
    canonical_path: &Path,
    legacy_file_name: &str,
) -> anyhow::Result<Option<String>> {
    if canonical_path.exists() {
        return Ok(Some(fs::read_to_string(canonical_path).await?));
    }
    let Some(legacy_path) = config::paths::resolve_legacy_root_file_path(legacy_file_name) else {
        return Ok(None);
    };
    if !legacy_path.exists() {
        return Ok(None);
    }
    Ok(Some(fs::read_to_string(legacy_path).await?))
}

fn parse_optimization_campaigns_file(
    raw: &str,
) -> std::collections::HashMap<String, OptimizationCampaignRecord> {
    serde_json::from_str::<std::collections::HashMap<String, OptimizationCampaignRecord>>(raw)
        .unwrap_or_default()
}

fn parse_optimization_experiments_file(
    raw: &str,
) -> std::collections::HashMap<String, OptimizationExperimentRecord> {
    serde_json::from_str::<std::collections::HashMap<String, OptimizationExperimentRecord>>(raw)
        .unwrap_or_default()
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

fn cron_expression_for_schedule_parser(expression: &str) -> String {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() == 5 {
        format!("0 {}", fields.join(" "))
    } else {
        expression.to_string()
    }
}

fn next_cron_fire_at_ms(expression: &str, timezone: &str, from_ms: u64) -> Option<u64> {
    let tz = parse_timezone(timezone)?;
    let parser_expression = cron_expression_for_schedule_parser(expression);
    let schedule = Schedule::from_str(&parser_expression).ok()?;
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

    #[track_caller]
    fn deref(&self) -> &Self::Target {
        if let Some(runtime) = self.runtime.get() {
            return runtime;
        }
        let caller = std::panic::Location::caller();
        tracing::error!(
            file = caller.file(),
            line = caller.line(),
            column = caller.column(),
            "runtime accessed before startup completion"
        );
        panic!("runtime accessed before startup completion")
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

pub fn extract_persistable_tool_part(properties: &Value) -> Option<(String, MessagePart)> {
    let part = properties.get("part")?;
    let part_type = part
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if part_type != "tool"
        && part_type != "tool-invocation"
        && part_type != "tool-result"
        && part_type != "tool_invocation"
        && part_type != "tool_result"
    {
        return None;
    }
    let part_state = part
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let has_result = part.get("result").is_some_and(|value| !value.is_null());
    let has_error = part
        .get("error")
        .and_then(|v| v.as_str())
        .is_some_and(|value| !value.trim().is_empty());
    // Skip transient "running" deltas to avoid persistence storms from streamed
    // tool-argument chunks; keep actionable/final updates.
    if part_state == "running" && !has_result && !has_error {
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
        if args.is_null() || args.as_object().is_some_and(|value| value.is_empty()) {
            if let Some(raw_preview) = properties
                .get("toolCallDelta")
                .and_then(|delta| delta.get("rawArgsPreview"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                args = Value::String(raw_preview.to_string());
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

pub fn derive_status_index_update(event: &EngineEvent) -> Option<StatusIndexUpdate> {
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
            let part = event.properties.get("part")?;
            let part_type = part.get("type").and_then(|v| v.as_str())?;
            let part_state = part.get("state").and_then(|v| v.as_str()).unwrap_or("");
            let (phase, tool_active) = match (part_type, part_state) {
                ("tool-invocation", _) | ("tool", "running") | ("tool", "") => ("tool", true),
                ("tool-result", _) | ("tool", "completed") | ("tool", "failed") => ("run", false),
                _ => return None,
            };
            base.insert("state".to_string(), Value::String("running".to_string()));
            base.insert("phase".to_string(), Value::String(phase.to_string()));
            base.insert("toolActive".to_string(), Value::Bool(tool_active));
            if let Some(tool_name) = part.get("tool").and_then(|v| v.as_str()) {
                base.insert("tool".to_string(), Value::String(tool_name.to_string()));
            }
            if let Some(tool_state) = part.get("state").and_then(|v| v.as_str()) {
                base.insert(
                    "toolState".to_string(),
                    Value::String(tool_state.to_string()),
                );
            }
            if let Some(tool_error) = part
                .get("error")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                base.insert(
                    "toolError".to_string(),
                    Value::String(tool_error.to_string()),
                );
            }
            if let Some(tool_call_id) = part
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                base.insert(
                    "toolCallID".to_string(),
                    Value::String(tool_call_id.to_string()),
                );
            }
            if let Some(args_preview) = part
                .get("args")
                .filter(|value| {
                    !value.is_null()
                        && !value.as_object().is_some_and(|map| map.is_empty())
                        && !value
                            .as_str()
                            .map(|text| text.trim().is_empty())
                            .unwrap_or(false)
                })
                .map(|value| truncate_text(&value.to_string(), 500))
            {
                base.insert(
                    "toolArgsPreview".to_string(),
                    Value::String(args_preview.to_string()),
                );
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

include!("app_state_impl_parts/part06.rs");
include!("app_state_impl_parts/part07.rs");
include!("app_state_impl_parts/part08.rs");
include!("app_state_impl_parts/part09.rs");

// NOTE: submodule declarations must live in this module-root file (`mod X;` path
// resolution is relative to the declaring file), so they stay here rather than in
// an included part.
pub mod automation;
pub use automation::*;

mod automation_v2_context_recovery;

pub mod principals;

#[cfg(test)]
mod tests;
