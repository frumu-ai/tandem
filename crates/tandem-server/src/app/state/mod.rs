use crate::config::channels::normalize_allowed_tools;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_memory::types::MemoryTier;
use tandem_orchestrator::MissionState;
use tandem_types::{EngineEvent, HostRuntimeContext, MessagePart, ModelSpec};
use tokio::fs;
use tokio::sync::RwLock;

use tandem_channels::config::{ChannelsConfig, DiscordConfig, SlackConfig, TelegramConfig};
use tandem_core::{resolve_shared_paths, PromptContextHook, PromptContextHookContext};
use tandem_memory::db::MemoryDatabase;
use tandem_providers::ChatMessage;
use tandem_workflows::{
    load_registry as load_workflow_registry, validate_registry as validate_workflow_registry,
    WorkflowHookBinding, WorkflowLoadSource, WorkflowRegistry, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowSourceKind, WorkflowSpec, WorkflowValidationMessage,
};

use crate::agent_teams::AgentTeamRuntime;
use crate::app::startup::{StartupSnapshot, StartupState, StartupStatus};
use crate::automation_v2::types::*;
use crate::bug_monitor::types::*;
use crate::capability_resolver::CapabilityResolver;
use crate::config::{self, channels::ChannelsConfigFile, webui::WebUiConfig};
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
    derive_phase1_metrics_from_run, establish_phase1_baseline, evaluate_phase1_promotion,
    optimization_snapshot_hash, parse_phase1_metrics, phase1_baseline_replay_due,
    validate_phase1_candidate_mutation, OptimizationBaselineReplayRecord,
    OptimizationCampaignRecord, OptimizationCampaignStatus, OptimizationExperimentRecord,
    OptimizationExperimentStatus, OptimizationMutableField, OptimizationPromotionDecisionKind,
};

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
    pub optimization_campaigns:
        Arc<RwLock<std::collections::HashMap<String, OptimizationCampaignRecord>>>,
    pub optimization_experiments:
        Arc<RwLock<std::collections::HashMap<String, OptimizationExperimentRecord>>>,
    pub bug_monitor_config: Arc<RwLock<BugMonitorConfig>>,
    pub bug_monitor_drafts: Arc<RwLock<std::collections::HashMap<String, BugMonitorDraftRecord>>>,
    pub bug_monitor_incidents:
        Arc<RwLock<std::collections::HashMap<String, BugMonitorIncidentRecord>>>,
    pub bug_monitor_posts: Arc<RwLock<std::collections::HashMap<String, BugMonitorPostRecord>>>,
    pub external_actions: Arc<RwLock<std::collections::HashMap<String, ExternalActionRecord>>>,
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
    pub optimization_campaigns_path: PathBuf,
    pub optimization_experiments_path: PathBuf,
    pub bug_monitor_config_path: PathBuf,
    pub bug_monitor_drafts_path: PathBuf,
    pub bug_monitor_incidents_path: PathBuf,
    pub bug_monitor_posts_path: PathBuf,
    pub external_actions_path: PathBuf,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelStatus {
    pub enabled: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub active_sessions: u64,
    pub meta: Value,
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
pub struct StatusIndexUpdate {
    pub key: String,
    pub value: Value,
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
            managed_worktrees: Arc::new(RwLock::new(std::collections::HashMap::new())),
            run_registry: RunRegistry::new(),
            run_stale_ms: config::env::resolve_run_stale_ms(),
            memory_records: Arc::new(RwLock::new(std::collections::HashMap::new())),
            memory_audit_log: Arc::new(RwLock::new(Vec::new())),
            missions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            shared_resources: Arc::new(RwLock::new(std::collections::HashMap::new())),
            shared_resources_path: config::paths::resolve_shared_resources_path(),
            routines: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_history: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automations_v2: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automation_v2_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_plans: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_plan_drafts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            optimization_campaigns: Arc::new(RwLock::new(std::collections::HashMap::new())),
            optimization_experiments: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_config: Arc::new(
                RwLock::new(config::env::resolve_bug_monitor_env_config()),
            ),
            bug_monitor_drafts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_incidents: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_posts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            external_actions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_runtime_status: Arc::new(RwLock::new(BugMonitorRuntimeStatus::default())),
            workflows: Arc::new(RwLock::new(WorkflowRegistry::default())),
            workflow_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_hook_overrides: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_dispatch_seen: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_session_policies: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automation_v2_session_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routines_path: config::paths::resolve_routines_path(),
            routine_history_path: config::paths::resolve_routine_history_path(),
            routine_runs_path: config::paths::resolve_routine_runs_path(),
            automations_v2_path: config::paths::resolve_automations_v2_path(),
            automation_v2_runs_path: config::paths::resolve_automation_v2_runs_path(),
            optimization_campaigns_path: config::paths::resolve_optimization_campaigns_path(),
            optimization_experiments_path: config::paths::resolve_optimization_experiments_path(),
            bug_monitor_config_path: config::paths::resolve_bug_monitor_config_path(),
            bug_monitor_drafts_path: config::paths::resolve_bug_monitor_drafts_path(),
            bug_monitor_incidents_path: config::paths::resolve_bug_monitor_incidents_path(),
            bug_monitor_posts_path: config::paths::resolve_bug_monitor_posts_path(),
            external_actions_path: config::paths::resolve_external_actions_path(),
            workflow_runs_path: config::paths::resolve_workflow_runs_path(),
            workflow_hook_overrides_path: config::paths::resolve_workflow_hook_overrides_path(),
            agent_teams: AgentTeamRuntime::new(config::paths::resolve_agent_team_audit_path()),
            web_ui_enabled: Arc::new(AtomicBool::new(false)),
            web_ui_prefix: Arc::new(std::sync::RwLock::new("/admin".to_string())),
            server_base_url: Arc::new(std::sync::RwLock::new("http://127.0.0.1:39731".to_string())),
            channels_runtime: Arc::new(tokio::sync::Mutex::new(ChannelRuntime::default())),
            host_runtime_context: detect_host_runtime_context(),
            token_cost_per_1k_usd: config::env::resolve_token_cost_per_1k_usd(),
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
            *guard = config::webui::normalize_web_ui_prefix(&prefix);
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
        #[cfg(feature = "browser")]
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
        let _ = self.load_optimization_campaigns().await;
        let _ = self.load_optimization_experiments().await;
        let _ = self.load_bug_monitor_config().await;
        let _ = self.load_bug_monitor_drafts().await;
        let _ = self.load_bug_monitor_incidents().await;
        let _ = self.load_bug_monitor_posts().await;
        let _ = self.load_external_actions().await;
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
                let backup_path = config::paths::sibling_backup_path(&self.routines_path);
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
        let backup_path = config::paths::sibling_backup_path(&self.routines_path);
        if self.routines_path.exists() {
            let _ = fs::copy(&self.routines_path, &backup_path).await;
        }
        let tmp_path = config::paths::sibling_tmp_path(&self.routines_path);
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

        routine.allowed_tools = config::channels::normalize_allowed_tools(routine.allowed_tools);
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
            allowed_tools: config::channels::normalize_allowed_tools(allowed_tools),
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
        let mut migrated = false;
        let mut path_counts = Vec::new();
        let mut canonical_loaded = false;
        if self.automations_v2_path.exists() {
            let raw = fs::read_to_string(&self.automations_v2_path).await?;
            if raw.trim().is_empty() || raw.trim() == "{}" {
                path_counts.push((self.automations_v2_path.clone(), 0usize));
            } else {
                let parsed = parse_automation_v2_file(&raw);
                path_counts.push((self.automations_v2_path.clone(), parsed.len()));
                canonical_loaded = !parsed.is_empty();
                merged = parsed;
            }
        } else {
            path_counts.push((self.automations_v2_path.clone(), 0usize));
        }
        if !canonical_loaded {
            for path in candidate_automations_v2_paths(&self.automations_v2_path) {
                if path == self.automations_v2_path {
                    continue;
                }
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
                if !parsed.is_empty() {
                    loaded_from_alternate = true;
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
        } else {
            for path in candidate_automations_v2_paths(&self.automations_v2_path) {
                if path == self.automations_v2_path {
                    continue;
                }
                if !path.exists() {
                    path_counts.push((path, 0usize));
                    continue;
                }
                let raw = fs::read_to_string(&path).await?;
                let count = if raw.trim().is_empty() || raw.trim() == "{}" {
                    0usize
                } else {
                    parse_automation_v2_file(&raw).len()
                };
                path_counts.push((path, count));
            }
        }
        let active_path = self.automations_v2_path.display().to_string();
        let path_count_summary = path_counts
            .iter()
            .map(|(path, count)| format!("{}={count}", path.display()))
            .collect::<Vec<_>>();
        tracing::info!(
            active_path,
            canonical_loaded,
            path_counts = ?path_count_summary,
            merged_count = merged.len(),
            "loaded automation v2 definitions"
        );
        for automation in merged.values_mut() {
            migrated = migrate_bundled_studio_research_split_automation(automation) || migrated;
        }
        *self.automations_v2.write().await = merged;
        if loaded_from_alternate || migrated {
            let _ = self.persist_automations_v2().await;
        } else if canonical_loaded {
            let _ = cleanup_stale_legacy_automations_v2_file(&self.automations_v2_path).await;
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
        let _ = cleanup_stale_legacy_automations_v2_file(&self.automations_v2_path).await;
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

    pub async fn load_optimization_campaigns(&self) -> anyhow::Result<()> {
        if !self.optimization_campaigns_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.optimization_campaigns_path).await?;
        let parsed = parse_optimization_campaigns_file(&raw);
        *self.optimization_campaigns.write().await = parsed;
        Ok(())
    }

    pub async fn persist_optimization_campaigns(&self) -> anyhow::Result<()> {
        let payload = {
            let guard = self.optimization_campaigns.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.optimization_campaigns_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.optimization_campaigns_path, payload).await?;
        Ok(())
    }

    pub async fn load_optimization_experiments(&self) -> anyhow::Result<()> {
        if !self.optimization_experiments_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.optimization_experiments_path).await?;
        let parsed = parse_optimization_experiments_file(&raw);
        *self.optimization_experiments.write().await = parsed;
        Ok(())
    }

    pub async fn persist_optimization_experiments(&self) -> anyhow::Result<()> {
        let payload = {
            let guard = self.optimization_experiments.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.optimization_experiments_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.optimization_experiments_path, payload).await?;
        Ok(())
    }

    async fn verify_automation_v2_persisted(
        &self,
        automation_id: &str,
        expected_present: bool,
    ) -> anyhow::Result<()> {
        let active_raw = if self.automations_v2_path.exists() {
            fs::read_to_string(&self.automations_v2_path).await?
        } else {
            String::new()
        };
        let active_parsed = parse_automation_v2_file(&active_raw);
        let active_present = active_parsed.contains_key(automation_id);
        if active_present != expected_present {
            let active_path = self.automations_v2_path.display().to_string();
            tracing::error!(
                automation_id,
                expected_present,
                actual_present = active_present,
                count = active_parsed.len(),
                active_path,
                "automation v2 persistence verification failed"
            );
            anyhow::bail!(
                "automation v2 persistence verification failed for `{}`",
                automation_id
            );
        }
        let mut alternate_mismatches = Vec::new();
        for path in candidate_automations_v2_paths(&self.automations_v2_path) {
            if path == self.automations_v2_path {
                continue;
            }
            let raw = if path.exists() {
                fs::read_to_string(&path).await?
            } else {
                String::new()
            };
            let parsed = parse_automation_v2_file(&raw);
            let present = parsed.contains_key(automation_id);
            if present != expected_present {
                alternate_mismatches.push(format!(
                    "{} expected_present={} actual_present={} count={}",
                    path.display(),
                    expected_present,
                    present,
                    parsed.len()
                ));
            }
        }
        if !alternate_mismatches.is_empty() {
            let active_path = self.automations_v2_path.display().to_string();
            tracing::warn!(
                automation_id,
                expected_present,
                mismatches = ?alternate_mismatches,
                active_path,
                "automation v2 alternate persistence paths are stale"
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
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed = serde_json::from_str::<BugMonitorConfig>(&raw)
            .unwrap_or_else(|_| config::env::resolve_bug_monitor_env_config());
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
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
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
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
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
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
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

    pub async fn load_external_actions(&self) -> anyhow::Result<()> {
        if !self.external_actions_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.external_actions_path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, ExternalActionRecord>>(&raw)
                .unwrap_or_default();
        *self.external_actions.write().await = parsed;
        Ok(())
    }

    pub async fn persist_external_actions(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.external_actions_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.external_actions.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.external_actions_path, payload).await?;
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

    pub async fn list_external_actions(&self, limit: usize) -> Vec<ExternalActionRecord> {
        let mut rows = self
            .external_actions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_external_action(&self, action_id: &str) -> Option<ExternalActionRecord> {
        self.external_actions.read().await.get(action_id).cloned()
    }

    pub async fn put_external_action(
        &self,
        action: ExternalActionRecord,
    ) -> anyhow::Result<ExternalActionRecord> {
        self.external_actions
            .write()
            .await
            .insert(action.action_id.clone(), action.clone());
        self.persist_external_actions().await?;
        Ok(action)
    }

    pub async fn record_external_action(
        &self,
        action: ExternalActionRecord,
    ) -> anyhow::Result<ExternalActionRecord> {
        let action = self.put_external_action(action).await?;
        if let Some(run_id) = action.routine_run_id.as_deref() {
            let artifact = RoutineRunArtifact {
                artifact_id: format!("external-action-{}", action.action_id),
                uri: format!("external-action://{}", action.action_id),
                kind: "external_action_receipt".to_string(),
                label: Some(format!("external action receipt: {}", action.operation)),
                created_at_ms: action.updated_at_ms,
                metadata: Some(json!({
                    "actionID": action.action_id,
                    "operation": action.operation,
                    "status": action.status,
                    "sourceKind": action.source_kind,
                    "sourceID": action.source_id,
                    "capabilityID": action.capability_id,
                    "target": action.target,
                })),
            };
            let _ = self
                .append_routine_run_artifact(run_id, artifact.clone())
                .await;
            if let Some(runtime) = self.runtime.get() {
                runtime.event_bus.publish(EngineEvent::new(
                    "routine.run.artifact_added",
                    json!({
                        "runID": run_id,
                        "artifact": artifact,
                    }),
                ));
            }
        }
        if let Some(context_run_id) = action.context_run_id.as_deref() {
            let payload = serde_json::to_value(&action)?;
            if let Err(error) = crate::http::context_runs::append_json_artifact_to_context_run(
                self,
                context_run_id,
                &format!("external-action-{}", action.action_id),
                "external_action_receipt",
                &format!("external-actions/{}.json", action.action_id),
                &payload,
            )
            .await
            {
                tracing::warn!(
                    "failed to append external action artifact {} to context run {}: {}",
                    action.action_id,
                    context_run_id,
                    error
                );
            }
        }
        Ok(action)
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
            .and_then(crate::app::routines::parse_model_spec);
        let selected_model_ready = selected_model
            .as_ref()
            .map(|spec| crate::app::routines::provider_catalog_has_model(&provider_catalog, spec))
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
            root: config::paths::resolve_builtin_workflows_dir(),
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
                config::channels::normalize_allowed_tools(agent.tool_policy.allowlist.clone());
            agent.tool_policy.denylist =
                config::channels::normalize_allowed_tools(agent.tool_policy.denylist.clone());
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
        migrate_bundled_studio_research_split_automation(&mut automation);
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

    pub async fn put_optimization_campaign(
        &self,
        mut campaign: OptimizationCampaignRecord,
    ) -> anyhow::Result<OptimizationCampaignRecord> {
        if campaign.optimization_id.trim().is_empty() {
            anyhow::bail!("optimization_id is required");
        }
        if campaign.source_workflow_id.trim().is_empty() {
            anyhow::bail!("source_workflow_id is required");
        }
        if campaign.name.trim().is_empty() {
            anyhow::bail!("name is required");
        }
        let now = now_ms();
        if campaign.created_at_ms == 0 {
            campaign.created_at_ms = now;
        }
        campaign.updated_at_ms = now;
        campaign.source_workflow_snapshot_hash =
            optimization_snapshot_hash(&campaign.source_workflow_snapshot);
        campaign.baseline_snapshot_hash = optimization_snapshot_hash(&campaign.baseline_snapshot);
        self.optimization_campaigns
            .write()
            .await
            .insert(campaign.optimization_id.clone(), campaign.clone());
        self.persist_optimization_campaigns().await?;
        Ok(campaign)
    }

    pub async fn get_optimization_campaign(
        &self,
        optimization_id: &str,
    ) -> Option<OptimizationCampaignRecord> {
        self.optimization_campaigns
            .read()
            .await
            .get(optimization_id)
            .cloned()
    }

    pub async fn list_optimization_campaigns(&self) -> Vec<OptimizationCampaignRecord> {
        let mut rows = self
            .optimization_campaigns
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub async fn put_optimization_experiment(
        &self,
        mut experiment: OptimizationExperimentRecord,
    ) -> anyhow::Result<OptimizationExperimentRecord> {
        if experiment.experiment_id.trim().is_empty() {
            anyhow::bail!("experiment_id is required");
        }
        if experiment.optimization_id.trim().is_empty() {
            anyhow::bail!("optimization_id is required");
        }
        let now = now_ms();
        if experiment.created_at_ms == 0 {
            experiment.created_at_ms = now;
        }
        experiment.updated_at_ms = now;
        experiment.candidate_snapshot_hash =
            optimization_snapshot_hash(&experiment.candidate_snapshot);
        self.optimization_experiments
            .write()
            .await
            .insert(experiment.experiment_id.clone(), experiment.clone());
        self.persist_optimization_experiments().await?;
        Ok(experiment)
    }

    pub async fn get_optimization_experiment(
        &self,
        optimization_id: &str,
        experiment_id: &str,
    ) -> Option<OptimizationExperimentRecord> {
        self.optimization_experiments
            .read()
            .await
            .get(experiment_id)
            .filter(|row| row.optimization_id == optimization_id)
            .cloned()
    }

    pub async fn list_optimization_experiments(
        &self,
        optimization_id: &str,
    ) -> Vec<OptimizationExperimentRecord> {
        let mut rows = self
            .optimization_experiments
            .read()
            .await
            .values()
            .filter(|row| row.optimization_id == optimization_id)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub async fn count_optimization_experiments(&self, optimization_id: &str) -> usize {
        self.optimization_experiments
            .read()
            .await
            .values()
            .filter(|row| row.optimization_id == optimization_id)
            .count()
    }

    fn automation_run_is_terminal(status: &crate::AutomationRunStatus) -> bool {
        matches!(
            status,
            crate::AutomationRunStatus::Completed
                | crate::AutomationRunStatus::Blocked
                | crate::AutomationRunStatus::Failed
                | crate::AutomationRunStatus::Cancelled
        )
    }

    fn optimization_mutation_field_path(field: OptimizationMutableField) -> &'static str {
        match field {
            OptimizationMutableField::Objective => "objective",
            OptimizationMutableField::OutputContractSummaryGuidance => {
                "output_contract.summary_guidance"
            }
            OptimizationMutableField::TimeoutMs => "timeout_ms",
            OptimizationMutableField::RetryPolicyMaxAttempts => "retry_policy.max_attempts",
            OptimizationMutableField::RetryPolicyRetries => "retry_policy.retries",
        }
    }

    fn optimization_node_field_value(
        node: &crate::AutomationFlowNode,
        field: OptimizationMutableField,
    ) -> Result<Value, String> {
        match field {
            OptimizationMutableField::Objective => Ok(Value::String(node.objective.clone())),
            OptimizationMutableField::OutputContractSummaryGuidance => node
                .output_contract
                .as_ref()
                .and_then(|contract| contract.summary_guidance.clone())
                .map(Value::String)
                .ok_or_else(|| {
                    format!(
                        "node `{}` is missing output_contract.summary_guidance",
                        node.node_id
                    )
                }),
            OptimizationMutableField::TimeoutMs => node
                .timeout_ms
                .map(|value| json!(value))
                .ok_or_else(|| format!("node `{}` is missing timeout_ms", node.node_id)),
            OptimizationMutableField::RetryPolicyMaxAttempts => node
                .retry_policy
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|policy| policy.get("max_attempts"))
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "node `{}` is missing retry_policy.max_attempts",
                        node.node_id
                    )
                }),
            OptimizationMutableField::RetryPolicyRetries => node
                .retry_policy
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|policy| policy.get("retries"))
                .cloned()
                .ok_or_else(|| format!("node `{}` is missing retry_policy.retries", node.node_id)),
        }
    }

    fn set_optimization_node_field_value(
        node: &mut crate::AutomationFlowNode,
        field: OptimizationMutableField,
        value: &Value,
    ) -> Result<(), String> {
        match field {
            OptimizationMutableField::Objective => {
                node.objective = value
                    .as_str()
                    .ok_or_else(|| "objective apply value must be a string".to_string())?
                    .to_string();
            }
            OptimizationMutableField::OutputContractSummaryGuidance => {
                let guidance = value
                    .as_str()
                    .ok_or_else(|| {
                        "output_contract.summary_guidance apply value must be a string".to_string()
                    })?
                    .to_string();
                let contract = node.output_contract.as_mut().ok_or_else(|| {
                    format!(
                        "node `{}` is missing output_contract for apply",
                        node.node_id
                    )
                })?;
                contract.summary_guidance = Some(guidance);
            }
            OptimizationMutableField::TimeoutMs => {
                node.timeout_ms = Some(
                    value
                        .as_u64()
                        .ok_or_else(|| "timeout_ms apply value must be an integer".to_string())?,
                );
            }
            OptimizationMutableField::RetryPolicyMaxAttempts => {
                let next = value.as_i64().ok_or_else(|| {
                    "retry_policy.max_attempts apply value must be an integer".to_string()
                })?;
                let policy = node.retry_policy.get_or_insert_with(|| json!({}));
                let object = policy.as_object_mut().ok_or_else(|| {
                    format!("node `{}` retry_policy must be a JSON object", node.node_id)
                })?;
                object.insert("max_attempts".to_string(), json!(next));
            }
            OptimizationMutableField::RetryPolicyRetries => {
                let next = value.as_i64().ok_or_else(|| {
                    "retry_policy.retries apply value must be an integer".to_string()
                })?;
                let policy = node.retry_policy.get_or_insert_with(|| json!({}));
                let object = policy.as_object_mut().ok_or_else(|| {
                    format!("node `{}` retry_policy must be a JSON object", node.node_id)
                })?;
                object.insert("retries".to_string(), json!(next));
            }
        }
        Ok(())
    }

    fn append_optimization_apply_metadata(
        metadata: Option<Value>,
        record: Value,
    ) -> Result<Option<Value>, String> {
        let mut root = match metadata {
            Some(Value::Object(map)) => map,
            Some(_) => return Err("automation metadata must be a JSON object".to_string()),
            None => serde_json::Map::new(),
        };
        let history = root
            .entry("optimization_apply_history".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(entries) = history.as_array_mut() else {
            return Err("optimization_apply_history metadata must be an array".to_string());
        };
        entries.push(record.clone());
        root.insert("last_optimization_apply".to_string(), record);
        Ok(Some(Value::Object(root)))
    }

    fn build_optimization_apply_patch(
        baseline: &crate::AutomationV2Spec,
        candidate: &crate::AutomationV2Spec,
        mutation: &crate::OptimizationValidatedMutation,
        approved_at_ms: u64,
    ) -> Result<Value, String> {
        let baseline_node = baseline
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == mutation.node_id)
            .ok_or_else(|| format!("baseline node `{}` not found", mutation.node_id))?;
        let candidate_node = candidate
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == mutation.node_id)
            .ok_or_else(|| format!("candidate node `{}` not found", mutation.node_id))?;
        let before = Self::optimization_node_field_value(baseline_node, mutation.field)?;
        let after = Self::optimization_node_field_value(candidate_node, mutation.field)?;
        Ok(json!({
            "node_id": mutation.node_id,
            "field": mutation.field,
            "field_path": Self::optimization_mutation_field_path(mutation.field),
            "expected_before": before,
            "apply_value": after,
            "approved_at_ms": approved_at_ms,
        }))
    }

    pub async fn apply_optimization_winner(
        &self,
        optimization_id: &str,
        experiment_id: &str,
    ) -> Result<
        (
            OptimizationCampaignRecord,
            OptimizationExperimentRecord,
            crate::AutomationV2Spec,
        ),
        String,
    > {
        let campaign = self
            .get_optimization_campaign(optimization_id)
            .await
            .ok_or_else(|| "optimization not found".to_string())?;
        let mut experiment = self
            .get_optimization_experiment(optimization_id, experiment_id)
            .await
            .ok_or_else(|| "experiment not found".to_string())?;
        if experiment.status != OptimizationExperimentStatus::PromotionApproved {
            return Err("only approved winner experiments may be applied".to_string());
        }
        if campaign.baseline_snapshot_hash != experiment.candidate_snapshot_hash {
            return Err(
                "only the latest approved winner may be applied to the live workflow".to_string(),
            );
        }
        let patch = experiment
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("apply_patch"))
            .cloned()
            .ok_or_else(|| "approved experiment is missing apply_patch metadata".to_string())?;
        let node_id = patch
            .get("node_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "apply_patch.node_id is required".to_string())?;
        let field: OptimizationMutableField = serde_json::from_value(
            patch
                .get("field")
                .cloned()
                .ok_or_else(|| "apply_patch.field is required".to_string())?,
        )
        .map_err(|error| format!("invalid apply_patch.field: {error}"))?;
        let expected_before = patch
            .get("expected_before")
            .cloned()
            .ok_or_else(|| "apply_patch.expected_before is required".to_string())?;
        let apply_value = patch
            .get("apply_value")
            .cloned()
            .ok_or_else(|| "apply_patch.apply_value is required".to_string())?;
        let mut live = self
            .get_automation_v2(&campaign.source_workflow_id)
            .await
            .ok_or_else(|| "source workflow not found".to_string())?;
        let current_value = {
            let live_node = live
                .flow
                .nodes
                .iter()
                .find(|node| node.node_id == node_id)
                .ok_or_else(|| format!("live workflow node `{node_id}` not found"))?;
            Self::optimization_node_field_value(live_node, field)?
        };
        if current_value != expected_before {
            return Err(format!(
                "live workflow drift detected for node `{node_id}` {}",
                Self::optimization_mutation_field_path(field)
            ));
        }
        let live_node = live
            .flow
            .nodes
            .iter_mut()
            .find(|node| node.node_id == node_id)
            .ok_or_else(|| format!("live workflow node `{node_id}` not found"))?;
        Self::set_optimization_node_field_value(live_node, field, &apply_value)?;
        let applied_at_ms = now_ms();
        let apply_record = json!({
            "optimization_id": campaign.optimization_id,
            "experiment_id": experiment.experiment_id,
            "node_id": node_id,
            "field": field,
            "field_path": Self::optimization_mutation_field_path(field),
            "previous_value": expected_before,
            "new_value": apply_value,
            "applied_at_ms": applied_at_ms,
        });
        live.metadata =
            Self::append_optimization_apply_metadata(live.metadata.clone(), apply_record)?;
        let stored_live = self
            .put_automation_v2(live)
            .await
            .map_err(|error| error.to_string())?;
        let mut metadata = match experiment.metadata.take() {
            Some(Value::Object(map)) => map,
            Some(_) => return Err("experiment metadata must be a JSON object".to_string()),
            None => serde_json::Map::new(),
        };
        metadata.insert(
            "applied_to_live".to_string(),
            json!({
                "automation_id": stored_live.automation_id,
                "applied_at_ms": applied_at_ms,
                "field": field,
                "node_id": node_id,
            }),
        );
        experiment.metadata = Some(Value::Object(metadata));
        let stored_experiment = self
            .put_optimization_experiment(experiment)
            .await
            .map_err(|error| error.to_string())?;
        Ok((campaign, stored_experiment, stored_live))
    }

    async fn reconcile_pending_baseline_replays(
        &self,
        campaign: &mut OptimizationCampaignRecord,
    ) -> Result<bool, String> {
        let Some(phase1) = campaign.phase1.as_ref() else {
            return Ok(false);
        };
        let mut changed = false;
        let mut remaining = Vec::new();
        for run_id in campaign.pending_baseline_run_ids.clone() {
            let Some(run) = self.get_automation_v2_run(&run_id).await else {
                campaign.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                campaign.last_pause_reason = Some(format!(
                    "baseline replay run `{run_id}` was not found during optimization reconciliation"
                ));
                changed = true;
                continue;
            };
            if !Self::automation_run_is_terminal(&run.status) {
                remaining.push(run_id);
                continue;
            }
            if run.status != crate::AutomationRunStatus::Completed {
                campaign.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                campaign.last_pause_reason = Some(format!(
                    "baseline replay run `{}` finished with status `{:?}`",
                    run.run_id, run.status
                ));
                changed = true;
                continue;
            }
            if run.automation_id != campaign.source_workflow_id {
                campaign.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                campaign.last_pause_reason = Some(
                    "baseline replay run must belong to the optimization source workflow"
                        .to_string(),
                );
                changed = true;
                continue;
            }
            let snapshot = run.automation_snapshot.as_ref().ok_or_else(|| {
                "baseline replay run must include an automation snapshot".to_string()
            })?;
            if optimization_snapshot_hash(snapshot) != campaign.baseline_snapshot_hash {
                campaign.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                campaign.last_pause_reason = Some(
                    "baseline replay run does not match the current campaign baseline snapshot"
                        .to_string(),
                );
                changed = true;
                continue;
            }
            let metrics =
                derive_phase1_metrics_from_run(&run, &campaign.baseline_snapshot, phase1)?;
            campaign
                .baseline_replays
                .push(OptimizationBaselineReplayRecord {
                    replay_id: format!("baseline-replay-{}", uuid::Uuid::new_v4()),
                    automation_run_id: Some(run.run_id.clone()),
                    phase1_metrics: metrics,
                    experiment_count_at_recording: self
                        .count_optimization_experiments(&campaign.optimization_id)
                        .await as u64,
                    recorded_at_ms: now_ms(),
                });
            changed = true;
        }
        if remaining != campaign.pending_baseline_run_ids {
            campaign.pending_baseline_run_ids = remaining;
            changed = true;
        }
        Ok(changed)
    }

    pub async fn reconcile_optimization_campaigns(&self) -> Result<usize, String> {
        let campaigns = self.list_optimization_campaigns().await;
        let mut updated = 0usize;
        for campaign in campaigns {
            let Some(mut latest) = self
                .get_optimization_campaign(&campaign.optimization_id)
                .await
            else {
                continue;
            };
            let Some(phase1) = latest.phase1.clone() else {
                continue;
            };
            let mut changed = self.reconcile_pending_baseline_replays(&mut latest).await?;
            let experiment_count = self
                .count_optimization_experiments(&latest.optimization_id)
                .await;
            if latest.pending_baseline_run_ids.is_empty() {
                if phase1_baseline_replay_due(
                    &latest.baseline_replays,
                    latest.pending_baseline_run_ids.len(),
                    &phase1,
                    experiment_count,
                    now_ms(),
                ) {
                    if self.maybe_queue_phase1_baseline_replay(&mut latest).await? {
                        latest.status = OptimizationCampaignStatus::Draft;
                        changed = true;
                    }
                } else if latest.baseline_replays.len()
                    >= phase1.eval.campaign_start_baseline_runs.max(1) as usize
                {
                    match establish_phase1_baseline(&latest.baseline_replays, &phase1) {
                        Ok(metrics) => {
                            if latest.baseline_metrics.as_ref() != Some(&metrics) {
                                latest.baseline_metrics = Some(metrics);
                                changed = true;
                            }
                            if latest.status != OptimizationCampaignStatus::Running
                                || latest.last_pause_reason.is_some()
                            {
                                latest.status = OptimizationCampaignStatus::Running;
                                latest.last_pause_reason = None;
                                changed = true;
                            }
                        }
                        Err(error) => {
                            if latest.status != OptimizationCampaignStatus::PausedEvaluatorUnstable
                                || latest.last_pause_reason.as_deref() != Some(error.as_str())
                            {
                                latest.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                                latest.last_pause_reason = Some(error);
                                changed = true;
                            }
                        }
                    }
                }
            } else if latest.last_pause_reason.as_deref()
                != Some("waiting for phase 1 baseline replay completion")
            {
                latest.last_pause_reason =
                    Some("waiting for phase 1 baseline replay completion".to_string());
                changed = true;
            }
            if changed {
                self.put_optimization_campaign(latest)
                    .await
                    .map_err(|error| error.to_string())?;
                updated = updated.saturating_add(1);
            }
        }
        Ok(updated)
    }

    async fn maybe_queue_phase1_baseline_replay(
        &self,
        campaign: &mut OptimizationCampaignRecord,
    ) -> Result<bool, String> {
        let Some(phase1) = campaign.phase1.as_ref() else {
            return Ok(false);
        };
        let required_runs = phase1.eval.campaign_start_baseline_runs.max(1) as usize;
        if !campaign.pending_baseline_run_ids.is_empty() {
            campaign.last_pause_reason =
                Some("waiting for phase 1 baseline replay completion".into());
            campaign.updated_at_ms = now_ms();
            return Ok(true);
        }
        if campaign.baseline_replays.len() >= required_runs {
            return Ok(false);
        }
        let replay_run = self
            .create_automation_v2_run(&campaign.baseline_snapshot, "optimization_baseline_replay")
            .await
            .map_err(|error| error.to_string())?;
        if !campaign
            .pending_baseline_run_ids
            .iter()
            .any(|value| value == &replay_run.run_id)
        {
            campaign
                .pending_baseline_run_ids
                .push(replay_run.run_id.clone());
        }
        campaign.last_pause_reason = Some("waiting for phase 1 baseline replay completion".into());
        campaign.updated_at_ms = now_ms();
        Ok(true)
    }

    pub async fn apply_optimization_action(
        &self,
        optimization_id: &str,
        action: &str,
        experiment_id: Option<&str>,
        run_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<OptimizationCampaignRecord, String> {
        let normalized = action.trim().to_ascii_lowercase();
        let mut campaign = self
            .get_optimization_campaign(optimization_id)
            .await
            .ok_or_else(|| "optimization not found".to_string())?;
        match normalized.as_str() {
            "start" => {
                if campaign.phase1.is_some() {
                    if self
                        .maybe_queue_phase1_baseline_replay(&mut campaign)
                        .await?
                    {
                        campaign.status = OptimizationCampaignStatus::Draft;
                    } else {
                        let phase1 = campaign
                            .phase1
                            .as_ref()
                            .ok_or_else(|| "phase 1 config is required".to_string())?;
                        match establish_phase1_baseline(&campaign.baseline_replays, phase1) {
                            Ok(metrics) => {
                                campaign.baseline_metrics = Some(metrics);
                                campaign.status = OptimizationCampaignStatus::Running;
                                campaign.last_pause_reason = None;
                            }
                            Err(error) => {
                                campaign.status =
                                    OptimizationCampaignStatus::PausedEvaluatorUnstable;
                                campaign.last_pause_reason = Some(error);
                            }
                        }
                    }
                } else {
                    campaign.status = OptimizationCampaignStatus::Running;
                    campaign.last_pause_reason = None;
                }
            }
            "pause" => {
                campaign.status = OptimizationCampaignStatus::PausedManual;
                campaign.last_pause_reason = reason
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
            "resume" => {
                if self
                    .maybe_queue_phase1_baseline_replay(&mut campaign)
                    .await?
                {
                    campaign.status = OptimizationCampaignStatus::Draft;
                } else {
                    campaign.status = OptimizationCampaignStatus::Running;
                    campaign.last_pause_reason = None;
                }
            }
            "queue_baseline_replay" => {
                let replay_run = self
                    .create_automation_v2_run(
                        &campaign.baseline_snapshot,
                        "optimization_baseline_replay",
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                if !campaign
                    .pending_baseline_run_ids
                    .iter()
                    .any(|value| value == &replay_run.run_id)
                {
                    campaign
                        .pending_baseline_run_ids
                        .push(replay_run.run_id.clone());
                }
                campaign.updated_at_ms = now_ms();
            }
            "record_baseline_replay" => {
                let run_id = run_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "run_id is required for record_baseline_replay".to_string())?;
                let phase1 = campaign
                    .phase1
                    .as_ref()
                    .ok_or_else(|| "phase 1 config is required for baseline replay".to_string())?;
                let run = self
                    .get_automation_v2_run(run_id)
                    .await
                    .ok_or_else(|| "automation run not found".to_string())?;
                if run.automation_id != campaign.source_workflow_id {
                    return Err(
                        "baseline replay run must belong to the optimization source workflow"
                            .to_string(),
                    );
                }
                let snapshot = run.automation_snapshot.as_ref().ok_or_else(|| {
                    "baseline replay run must include an automation snapshot".to_string()
                })?;
                if optimization_snapshot_hash(snapshot) != campaign.baseline_snapshot_hash {
                    return Err(
                        "baseline replay run does not match the current campaign baseline snapshot"
                            .to_string(),
                    );
                }
                let metrics =
                    derive_phase1_metrics_from_run(&run, &campaign.baseline_snapshot, phase1)?;
                campaign
                    .baseline_replays
                    .push(OptimizationBaselineReplayRecord {
                        replay_id: format!("baseline-replay-{}", uuid::Uuid::new_v4()),
                        automation_run_id: Some(run.run_id.clone()),
                        phase1_metrics: metrics,
                        experiment_count_at_recording: self
                            .count_optimization_experiments(&campaign.optimization_id)
                            .await as u64,
                        recorded_at_ms: now_ms(),
                    });
                campaign
                    .pending_baseline_run_ids
                    .retain(|value| value != run_id);
                campaign.updated_at_ms = now_ms();
            }
            "approve_winner" => {
                let experiment_id = experiment_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "experiment_id is required for approve_winner".to_string())?;
                let mut experiment = self
                    .get_optimization_experiment(optimization_id, experiment_id)
                    .await
                    .ok_or_else(|| "experiment not found".to_string())?;
                if experiment.baseline_snapshot_hash != campaign.baseline_snapshot_hash {
                    return Err(
                        "experiment baseline_snapshot_hash does not match current campaign baseline"
                            .to_string(),
                    );
                }
                if let Some(phase1) = campaign.phase1.as_ref() {
                    let validated = validate_phase1_candidate_mutation(
                        &campaign.baseline_snapshot,
                        &experiment.candidate_snapshot,
                        phase1,
                    )?;
                    if experiment.mutation_summary.is_none() {
                        experiment.mutation_summary = Some(validated.summary.clone());
                    }
                    let approved_at_ms = now_ms();
                    let apply_patch = Self::build_optimization_apply_patch(
                        &campaign.baseline_snapshot,
                        &experiment.candidate_snapshot,
                        &validated,
                        approved_at_ms,
                    )?;
                    let mut metadata = match experiment.metadata.take() {
                        Some(Value::Object(map)) => map,
                        Some(_) => {
                            return Err("experiment metadata must be a JSON object".to_string());
                        }
                        None => serde_json::Map::new(),
                    };
                    metadata.insert("apply_patch".to_string(), apply_patch);
                    experiment.metadata = Some(Value::Object(metadata));
                    if let Some(baseline_metrics) = campaign.baseline_metrics.as_ref() {
                        let candidate_metrics = experiment
                            .phase1_metrics
                            .clone()
                            .or_else(|| {
                                experiment
                                    .metrics
                                    .as_ref()
                                    .and_then(|metrics| parse_phase1_metrics(metrics).ok())
                            })
                            .ok_or_else(|| {
                                "phase 1 candidate is missing promotion metrics".to_string()
                            })?;
                        let decision =
                            evaluate_phase1_promotion(baseline_metrics, &candidate_metrics);
                        experiment.promotion_recommendation = Some(
                            match decision.decision {
                                OptimizationPromotionDecisionKind::Promote => "promote",
                                OptimizationPromotionDecisionKind::Discard => "discard",
                                OptimizationPromotionDecisionKind::NeedsOperatorReview => {
                                    "needs_operator_review"
                                }
                            }
                            .to_string(),
                        );
                        experiment.promotion_decision = Some(decision.clone());
                        if decision.decision != OptimizationPromotionDecisionKind::Promote {
                            let _ = self
                                .put_optimization_experiment(experiment)
                                .await
                                .map_err(|e| e.to_string())?;
                            return Err(decision.reason);
                        }
                        campaign.baseline_metrics = Some(candidate_metrics);
                    }
                }
                campaign.baseline_snapshot = experiment.candidate_snapshot.clone();
                campaign.baseline_snapshot_hash = experiment.candidate_snapshot_hash.clone();
                campaign.baseline_replays.clear();
                campaign.pending_baseline_run_ids.clear();
                campaign.pending_promotion_experiment_id = None;
                campaign.status = OptimizationCampaignStatus::Draft;
                campaign.last_pause_reason = None;
                experiment.status = OptimizationExperimentStatus::PromotionApproved;
                let _ = self
                    .put_optimization_experiment(experiment)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            "reject_winner" => {
                let experiment_id = experiment_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "experiment_id is required for reject_winner".to_string())?;
                let mut experiment = self
                    .get_optimization_experiment(optimization_id, experiment_id)
                    .await
                    .ok_or_else(|| "experiment not found".to_string())?;
                campaign.pending_promotion_experiment_id = None;
                campaign.status = OptimizationCampaignStatus::Draft;
                campaign.last_pause_reason = reason
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                experiment.status = OptimizationExperimentStatus::PromotionRejected;
                let _ = self
                    .put_optimization_experiment(experiment)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            _ => return Err("unsupported optimization action".to_string()),
        }
        self.put_optimization_campaign(campaign)
            .await
            .map_err(|e| e.to_string())
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
        let removed_run_count = {
            let mut runs = self.automation_v2_runs.write().await;
            let before = runs.len();
            runs.retain(|_, run| run.automation_id != automation_id);
            before.saturating_sub(runs.len())
        };
        self.persist_automations_v2().await?;
        if removed_run_count > 0 {
            self.persist_automation_v2_runs().await?;
        }
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
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes,
                node_outputs: std::collections::HashMap::new(),
                node_attempts: std::collections::HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            automation_snapshot: Some(automation.clone()),
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
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
        crate::http::context_runs::sync_automation_v2_run_blackboard(self, automation, &run)
            .await
            .map_err(|status| anyhow::anyhow!("failed to sync automation context run: {status}"))?;
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
                | AutomationRunStatus::Blocked
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
                row.latest_session_id = Some(session_id.to_string());
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

    pub async fn forget_automation_v2_sessions(&self, session_ids: &[String]) {
        let mut guard = self.automation_v2_session_runs.write().await;
        for session_id in session_ids {
            guard.remove(session_id);
        }
    }

    pub async fn add_automation_v2_instance(
        &self,
        run_id: &str,
        instance_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            if !row.active_instance_ids.iter().any(|id| id == instance_id) {
                row.active_instance_ids.push(instance_id.to_string());
            }
        })
        .await
    }

    pub async fn clear_automation_v2_instance(
        &self,
        run_id: &str,
        instance_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            row.active_instance_ids.retain(|id| id != instance_id);
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
            allowed_users: config::channels::normalize_allowed_users_or_wildcard(cfg.allowed_users),
            mention_only: cfg.mention_only,
            style_profile: cfg.style_profile,
        }),
        discord: channels.discord.clone().map(|cfg| DiscordConfig {
            bot_token: cfg.bot_token,
            guild_id: cfg.guild_id,
            allowed_users: config::channels::normalize_allowed_users_or_wildcard(cfg.allowed_users),
            mention_only: cfg.mention_only,
        }),
        slack: channels.slack.clone().map(|cfg| SlackConfig {
            bot_token: cfg.bot_token,
            channel_id: cfg.channel_id,
            allowed_users: config::channels::normalize_allowed_users_or_wildcard(cfg.allowed_users),
            mention_only: cfg.mention_only,
        }),
        server_base_url: state.server_base_url(),
        api_token: state.api_token().await.unwrap_or_default(),
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
    serde_json::from_str::<std::collections::HashMap<String, AutomationV2Spec>>(raw)
        .unwrap_or_default()
}

fn parse_automation_v2_runs_file(
    raw: &str,
) -> std::collections::HashMap<String, AutomationV2RunRecord> {
    serde_json::from_str::<std::collections::HashMap<String, AutomationV2RunRecord>>(raw)
        .unwrap_or_default()
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

    fn build_memory_scope_block(
        session_id: &str,
        project_id: Option<&str>,
        workspace_root: Option<&str>,
    ) -> String {
        let mut lines = vec![
            "<memory_scope>".to_string(),
            format!("- current_session_id: {}", session_id),
        ];
        if let Some(project_id) = project_id.map(str::trim).filter(|value| !value.is_empty()) {
            lines.push(format!("- current_project_id: {}", project_id));
        }
        if let Some(workspace_root) = workspace_root
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("- workspace_root: {}", workspace_root));
        }
        lines.push(
            "- default_memory_search_behavior: search current session, then current project/workspace, then global memory"
                .to_string(),
        );
        lines.push(
            "- use memory_search without IDs for normal recall; only pass tier/session_id/project_id when narrowing scope"
                .to_string(),
        );
        lines.push(
            "- when memory is sparse or stale, inspect the workspace with glob, grep, and read"
                .to_string(),
        );
        lines.push("</memory_scope>".to_string());
        lines.join("\n")
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
            if let Some(session) = this.state.storage.get_session(&ctx.session_id).await {
                messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: Self::build_memory_scope_block(
                        &ctx.session_id,
                        session.project_id.as_deref(),
                        session.workspace_root.as_deref(),
                    ),
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

pub fn extract_persistable_tool_part(properties: &Value) -> Option<(String, MessagePart)> {
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

pub async fn run_session_part_persister(state: AppState) {
    crate::app::tasks::run_session_part_persister(state).await
}

pub async fn run_status_indexer(state: AppState) {
    crate::app::tasks::run_status_indexer(state).await
}

pub async fn run_agent_team_supervisor(state: AppState) {
    crate::app::tasks::run_agent_team_supervisor(state).await
}

pub async fn run_bug_monitor(state: AppState) {
    crate::app::tasks::run_bug_monitor(state).await
}

pub async fn run_usage_aggregator(state: AppState) {
    crate::app::tasks::run_usage_aggregator(state).await
}

pub async fn run_optimization_scheduler(state: AppState) {
    crate::app::tasks::run_optimization_scheduler(state).await
}

pub async fn process_bug_monitor_event(
    state: &AppState,
    event: &EngineEvent,
    config: &BugMonitorConfig,
) -> anyhow::Result<BugMonitorIncidentRecord> {
    let submission =
        crate::bug_monitor::service::build_bug_monitor_submission_from_event(state, config, event)
            .await?;
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

pub fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

pub async fn run_routine_scheduler(state: AppState) {
    crate::app::tasks::run_routine_scheduler(state).await
}

pub async fn run_routine_executor(state: AppState) {
    crate::app::tasks::run_routine_executor(state).await
}

pub async fn build_routine_prompt(state: &AppState, run: &RoutineRunRecord) -> String {
    crate::app::routines::build_routine_prompt(state, run).await
}

pub fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut out = input[..max_len].to_string();
    out.push_str("...<truncated>");
    out
}

pub async fn append_configured_output_artifacts(state: &AppState, run: &RoutineRunRecord) {
    crate::app::routines::append_configured_output_artifacts(state, run).await
}

pub fn default_model_spec_from_effective_config(config: &Value) -> Option<ModelSpec> {
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

pub async fn resolve_routine_model_spec_for_run(
    state: &AppState,
    run: &RoutineRunRecord,
) -> (Option<ModelSpec>, String) {
    crate::app::routines::resolve_routine_model_spec_for_run(state, run).await
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

#[cfg(not(feature = "browser"))]
impl AppState {
    pub async fn close_browser_sessions_for_owner(&self, _owner_session_id: &str) -> usize {
        0
    }

    pub async fn close_all_browser_sessions(&self) -> usize {
        0
    }

    pub async fn browser_status(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false, "sidecar": { "found": false }, "browser": { "found": false } })
    }

    pub async fn browser_smoke_test(
        &self,
        _url: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("browser feature disabled")
    }

    pub async fn install_browser_sidecar(&self) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("browser feature disabled")
    }

    pub async fn browser_health_summary(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false })
    }
}

pub mod automation;
pub use automation::*;

#[cfg(test)]
mod tests;
