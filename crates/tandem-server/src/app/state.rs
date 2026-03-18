use crate::config::channels::normalize_allowed_tools;
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

use crate::agent_teams::{self, AgentTeamRuntime};
use crate::app::startup::{StartupSnapshot, StartupState, StartupStatus};
use crate::automation_v2::{self, types::*};
use crate::browser::{
    install_browser_sidecar, BrowserHealthSummary, BrowserSidecarInstallResult,
    BrowserSmokeTestResult, BrowserSubsystem,
};
use crate::bug_monitor::{self, types::*};
use crate::capability_resolver::{self, CapabilityResolver};
use crate::config::{
    self,
    channels::{ChannelsConfigFile, DiscordConfigFile, SlackConfigFile, TelegramConfigFile},
    webui::WebUiConfig,
};
use crate::http::{self, serve};
use crate::mcp_catalog;
use crate::memory::{
    self,
    types::{GovernedMemoryRecord, MemoryAuditEvent},
};
use crate::pack_builder;
use crate::pack_manager::{self, PackManager};
use crate::preset_composer::{self, PromptComposeInput};
use crate::preset_registry::{self, PresetRegistry};
use crate::preset_summary;
use crate::routines::{self, errors::RoutineStoreError, types::*};
use crate::runtime::{
    self,
    lease::EngineLease,
    runs::{ActiveRun, RunRegistry},
    state::RuntimeState,
    worktrees::ManagedWorktreeRecord,
};
use crate::shared_resources::{
    self,
    types::{ResourceConflict, ResourceStoreError, SharedResourceRecord},
};
use crate::util::{
    self,
    build::{binary_path_for_health, build_id},
    host::detect_host_runtime_context,
    time::now_ms,
};
use crate::workflows::{
    self, canonical_workflow_event_names, dispatch_workflow_event, execute_hook_binding,
    execute_workflow, parse_workflow_action, run_workflow_dispatcher, simulate_workflow_event,
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
        *self.automations_v2.write().await = merged;
        if loaded_from_alternate {
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

fn is_bug_monitor_candidate_event(event: &EngineEvent) -> bool {
    if event.event_type.starts_with("bug_monitor.") {
        return false;
    }
    matches!(
        event.event_type.as_str(),
        "context.task.failed" | "workflow.run.failed" | "routine.run.failed" | "session.error"
    )
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

pub async fn run_automation_v2_scheduler(state: AppState) {
    crate::app::tasks::run_automation_v2_scheduler(state).await
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

pub(crate) fn is_automation_approval_node(node: &AutomationFlowNode) -> bool {
    matches!(node.stage_kind, Some(AutomationNodeStageKind::Approval))
        || node
            .gate
            .as_ref()
            .map(|gate| gate.required)
            .unwrap_or(false)
}

pub(crate) fn automation_guardrail_failure(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<String> {
    if let Some(max_runtime_ms) = automation.execution.max_total_runtime_ms {
        if let Some(started_at_ms) = run.started_at_ms {
            let elapsed = now_ms().saturating_sub(started_at_ms);
            if elapsed >= max_runtime_ms {
                return Some(format!(
                    "run exceeded max_total_runtime_ms ({elapsed}/{max_runtime_ms})"
                ));
            }
        }
    }
    if let Some(max_total_tokens) = automation.execution.max_total_tokens {
        if run.total_tokens >= max_total_tokens {
            return Some(format!(
                "run exceeded max_total_tokens ({}/{})",
                run.total_tokens, max_total_tokens
            ));
        }
    }
    if let Some(max_total_cost_usd) = automation.execution.max_total_cost_usd {
        if run.estimated_cost_usd >= max_total_cost_usd {
            return Some(format!(
                "run exceeded max_total_cost_usd ({:.4}/{:.4})",
                run.estimated_cost_usd, max_total_cost_usd
            ));
        }
    }
    None
}

pub fn record_automation_lifecycle_event(
    run: &mut AutomationV2RunRecord,
    event: impl Into<String>,
    reason: Option<String>,
    stop_kind: Option<AutomationStopKind>,
) {
    record_automation_lifecycle_event_with_metadata(run, event, reason, stop_kind, None);
}

pub fn record_automation_lifecycle_event_with_metadata(
    run: &mut AutomationV2RunRecord,
    event: impl Into<String>,
    reason: Option<String>,
    stop_kind: Option<AutomationStopKind>,
    metadata: Option<Value>,
) {
    run.checkpoint
        .lifecycle_history
        .push(AutomationLifecycleRecord {
            event: event.into(),
            recorded_at_ms: now_ms(),
            reason,
            stop_kind,
            metadata,
        });
}

pub fn automation_lifecycle_event_metadata_for_node(
    node_id: &str,
    attempt: u32,
    session_id: Option<&str>,
    summary: &str,
    contract_kind: &str,
    workflow_class: &str,
    phase: &str,
    status: &str,
    failure_kind: Option<&str>,
) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert("node_id".to_string(), json!(node_id));
    map.insert("attempt".to_string(), json!(attempt));
    map.insert("summary".to_string(), json!(summary));
    map.insert("contract_kind".to_string(), json!(contract_kind));
    map.insert("workflow_class".to_string(), json!(workflow_class));
    map.insert("phase".to_string(), json!(phase));
    map.insert("status".to_string(), json!(status));
    map.insert("event_contract_version".to_string(), json!(1));
    if let Some(value) = session_id.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert("session_id".to_string(), json!(value));
    }
    if let Some(value) = failure_kind
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert("failure_kind".to_string(), json!(value));
    }
    map
}

pub fn record_automation_workflow_state_events(
    run: &mut AutomationV2RunRecord,
    node_id: &str,
    output: &Value,
    attempt: u32,
    session_id: Option<&str>,
    summary: &str,
    contract_kind: &str,
) {
    let workflow_class = output
        .get("workflow_class")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("artifact");
    let phase = output
        .get("phase")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let status = output
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let failure_kind = output
        .get("failure_kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let artifact_validation = output.get("artifact_validation");
    let base_reason = output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("rejected_artifact_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        });

    let mut base_metadata = automation_lifecycle_event_metadata_for_node(
        node_id,
        attempt,
        session_id,
        summary,
        contract_kind,
        workflow_class,
        phase,
        status,
        failure_kind,
    );
    if let Some(classification) = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        base_metadata.insert("blocking_classification".to_string(), json!(classification));
    }
    if let Some(actions) = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
    {
        base_metadata.insert(
            "required_next_tool_actions".to_string(),
            Value::Array(actions.clone()),
        );
    }
    record_automation_lifecycle_event_with_metadata(
        run,
        "workflow_state_changed",
        base_reason.clone(),
        None,
        Some(Value::Object(base_metadata.clone())),
    );

    if let Some(candidates) = artifact_validation
        .and_then(|value| value.get("artifact_candidates"))
        .and_then(Value::as_array)
    {
        for candidate in candidates {
            let mut metadata = base_metadata.clone();
            metadata.insert("candidate".to_string(), candidate.clone());
            record_automation_lifecycle_event_with_metadata(
                run,
                "artifact_candidate_written",
                None,
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(source) = artifact_validation
        .and_then(|value| value.get("accepted_candidate_source"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut metadata = base_metadata.clone();
        metadata.insert("accepted_candidate_source".to_string(), json!(source));
        record_automation_lifecycle_event_with_metadata(
            run,
            "artifact_accepted",
            None,
            None,
            Some(Value::Object(metadata)),
        );
    }

    if let Some(reason) = artifact_validation
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut metadata = base_metadata.clone();
        metadata.insert("rejected_artifact_reason".to_string(), json!(reason));
        record_automation_lifecycle_event_with_metadata(
            run,
            "artifact_rejected",
            Some(reason.to_string()),
            None,
            Some(Value::Object(metadata)),
        );
    }

    let repair_attempted = artifact_validation
        .and_then(|value| value.get("repair_attempted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| tandem_core::prewrite_repair_retry_max_attempts() as u32);
    let repair_succeeded = artifact_validation
        .and_then(|value| value.get("repair_succeeded"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if repair_attempted {
        let mut metadata = base_metadata.clone();
        metadata.insert("repair_attempt".to_string(), json!(repair_attempt));
        metadata.insert(
            "repair_attempts_remaining".to_string(),
            json!(repair_attempts_remaining),
        );
        metadata.insert("repair_succeeded".to_string(), json!(repair_succeeded));
        metadata.insert("repair_exhausted".to_string(), json!(repair_exhausted));
        record_automation_lifecycle_event_with_metadata(
            run,
            "repair_started",
            None,
            None,
            Some(Value::Object(metadata.clone())),
        );
        if !repair_succeeded {
            record_automation_lifecycle_event_with_metadata(
                run,
                "repair_exhausted",
                base_reason.clone(),
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(unmet_requirements) = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
    {
        if workflow_class == "research" {
            let mut metadata = base_metadata.clone();
            metadata.insert(
                "unmet_requirements".to_string(),
                Value::Array(unmet_requirements.clone()),
            );
            record_automation_lifecycle_event_with_metadata(
                run,
                "research_coverage_failed",
                base_reason.clone(),
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(verification) = artifact_validation.and_then(|value| value.get("verification")) {
        let expected = verification
            .get("verification_expected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let ran = verification
            .get("verification_ran")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let failed = verification
            .get("verification_failed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if expected {
            let mut metadata = base_metadata.clone();
            metadata.insert("verification".to_string(), verification.clone());
            record_automation_lifecycle_event_with_metadata(
                run,
                "verification_started",
                None,
                None,
                Some(Value::Object(metadata.clone())),
            );
            if failed {
                record_automation_lifecycle_event_with_metadata(
                    run,
                    "verification_failed",
                    base_reason.clone(),
                    None,
                    Some(Value::Object(metadata)),
                );
            } else if ran {
                record_automation_lifecycle_event_with_metadata(
                    run,
                    "verification_passed",
                    None,
                    None,
                    Some(Value::Object(metadata)),
                );
            }
        }
    }
}

pub(crate) fn automation_output_session_id(output: &Value) -> Option<String> {
    output
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| {
            content
                .get("session_id")
                .or_else(|| content.get("sessionId"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn build_automation_pending_gate(
    node: &AutomationFlowNode,
) -> Option<AutomationPendingGate> {
    let gate = node.gate.as_ref()?;
    Some(AutomationPendingGate {
        node_id: node.node_id.clone(),
        title: node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("title"))
            .and_then(Value::as_str)
            .unwrap_or(node.objective.as_str())
            .to_string(),
        instructions: gate.instructions.clone(),
        decisions: gate.decisions.clone(),
        rework_targets: gate.rework_targets.clone(),
        requested_at_ms: now_ms(),
        upstream_node_ids: node.depends_on.clone(),
    })
}

fn automation_node_builder_metadata(node: &AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn automation_node_builder_priority(node: &AutomationFlowNode) -> i32 {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("priority"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0)
}

fn automation_phase_execution_mode_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, String> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .filter_map(|phase| {
                    let phase_id = phase.get("phase_id").and_then(Value::as_str)?.trim();
                    if phase_id.is_empty() {
                        return None;
                    }
                    let mode = phase
                        .get("execution_mode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("soft");
                    Some((phase_id.to_string(), mode.to_string()))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_current_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<(String, usize, String)> {
    let phase_rank = automation_phase_rank_map(automation);
    if phase_rank.is_empty() {
        return None;
    }
    let phase_modes = automation_phase_execution_mode_map(automation);
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| !completed.contains(&node.node_id))
        .filter_map(|node| {
            automation_node_builder_metadata(node, "phase_id").and_then(|phase_id| {
                phase_rank
                    .get(&phase_id)
                    .copied()
                    .map(|rank| (phase_id, rank))
            })
        })
        .min_by_key(|(_, rank)| *rank)
        .map(|(phase_id, rank)| {
            let mode = phase_modes
                .get(&phase_id)
                .cloned()
                .unwrap_or_else(|| "soft".to_string());
            (phase_id, rank, mode)
        })
}

pub(crate) fn automation_phase_rank_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, usize> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .enumerate()
                .filter_map(|(index, phase)| {
                    phase
                        .get("phase_id")
                        .and_then(Value::as_str)
                        .map(|phase_id| (phase_id.to_string(), index))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_node_sort_key(
    node: &AutomationFlowNode,
    phase_rank: &std::collections::HashMap<String, usize>,
    current_open_phase_rank: Option<usize>,
) -> (usize, usize, i32, String) {
    let phase_order = automation_node_builder_metadata(node, "phase_id")
        .as_ref()
        .and_then(|phase_id| phase_rank.get(phase_id))
        .copied()
        .unwrap_or(usize::MAX / 2);
    let open_phase_bias = current_open_phase_rank
        .map(|open_rank| usize::from(phase_order != open_rank))
        .unwrap_or(0);
    (
        open_phase_bias,
        phase_order,
        -automation_node_builder_priority(node),
        node.node_id.clone(),
    )
}

pub(crate) fn automation_filter_runnable_by_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    runnable: Vec<AutomationFlowNode>,
) -> Vec<AutomationFlowNode> {
    let Some((_, open_rank, _)) = automation_current_open_phase(automation, run) else {
        return runnable;
    };
    let phase_rank = automation_phase_rank_map(automation);
    let in_open_phase = runnable
        .iter()
        .filter(|node| {
            automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied()
                == Some(open_rank)
        })
        .cloned()
        .collect::<Vec<_>>();
    if in_open_phase.is_empty() {
        runnable
    } else {
        in_open_phase
    }
}

fn normalize_write_scope_entries(scope: Option<String>) -> Vec<String> {
    let Some(scope) = scope else {
        return vec!["__repo__".to_string()];
    };
    let entries = scope
        .split(|ch| matches!(ch, ',' | '\n' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('/').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        vec!["__repo__".to_string()]
    } else {
        entries
    }
}

fn write_scope_entries_conflict(left: &[String], right: &[String]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            a == "__repo__"
                || b == "__repo__"
                || a == b
                || a == "."
                || b == "."
                || a == "*"
                || b == "*"
                || a.starts_with(&format!("{}/", b))
                || b.starts_with(&format!("{}/", a))
        })
    })
}

pub(crate) fn automation_filter_runnable_by_write_scope_conflicts(
    runnable: Vec<AutomationFlowNode>,
    max_parallel: usize,
) -> Vec<AutomationFlowNode> {
    if max_parallel <= 1 {
        return runnable.into_iter().take(1).collect();
    }
    let mut selected = Vec::new();
    let mut selected_scopes = Vec::<Vec<String>>::new();
    for node in runnable {
        let is_code = automation_node_is_code_workflow(&node);
        let scope_entries = if is_code {
            normalize_write_scope_entries(automation_node_write_scope(&node))
        } else {
            Vec::new()
        };
        let conflicts = is_code
            && selected.iter().enumerate().any(|(index, existing)| {
                automation_node_is_code_workflow(existing)
                    && write_scope_entries_conflict(&scope_entries, &selected_scopes[index])
            });
        if conflicts {
            continue;
        }
        if is_code {
            selected_scopes.push(scope_entries);
        } else {
            selected_scopes.push(Vec::new());
        }
        selected.push(node);
        if selected.len() >= max_parallel {
            break;
        }
    }
    selected
}

pub(crate) fn automation_blocked_nodes(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Vec<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let pending = run
        .checkpoint
        .pending_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let phase_rank = automation_phase_rank_map(automation);
    let current_open_phase = automation_current_open_phase(automation, run);
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| pending.contains(&node.node_id))
        .filter_map(|node| {
            let missing_deps = node.depends_on.iter().any(|dep| !completed.contains(dep));
            if missing_deps {
                return Some(node.node_id.clone());
            }
            let Some((_, open_rank, mode)) = current_open_phase.as_ref() else {
                return None;
            };
            if mode != "barrier" {
                return None;
            }
            let node_phase_rank = automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied();
            if node_phase_rank.is_some_and(|rank| rank > *open_rank) {
                return Some(node.node_id.clone());
            }
            None
        })
        .collect::<Vec<_>>()
}

pub(crate) fn record_automation_open_phase_event(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    let Some((phase_id, phase_rank, execution_mode)) =
        automation_current_open_phase(automation, run)
    else {
        return;
    };
    let last_recorded = run
        .checkpoint
        .lifecycle_history
        .iter()
        .rev()
        .find(|entry| entry.event == "phase_opened")
        .and_then(|entry| entry.metadata.as_ref())
        .and_then(|metadata| metadata.get("phase_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if last_recorded.as_deref() == Some(phase_id.as_str()) {
        return;
    }
    record_automation_lifecycle_event_with_metadata(
        run,
        "phase_opened",
        Some(format!("phase `{}` is now open", phase_id)),
        None,
        Some(json!({
            "phase_id": phase_id,
            "phase_rank": phase_rank,
            "execution_mode": execution_mode,
        })),
    );
}

pub fn refresh_automation_runtime_state(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    run.checkpoint.blocked_nodes = automation_blocked_nodes(automation, run);
    record_automation_open_phase_event(automation, run);
}

fn automation_mission_milestones(automation: &AutomationV2Spec) -> Vec<Value> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("milestones"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn completed_mission_milestones(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> std::collections::HashSet<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation_mission_milestones(automation)
        .iter()
        .filter_map(|milestone| {
            let milestone_id = milestone
                .get("milestone_id")
                .and_then(Value::as_str)?
                .trim();
            if milestone_id.is_empty() {
                return None;
            }
            let required = milestone
                .get("required_stage_ids")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (!required.is_empty()
                && required
                    .iter()
                    .all(|stage_id| completed.contains(*stage_id)))
            .then_some(milestone_id.to_string())
        })
        .collect()
}

pub(crate) fn record_milestone_promotions(
    automation: &AutomationV2Spec,
    row: &mut AutomationV2RunRecord,
    promoted_by_node_id: &str,
) {
    let already_recorded = row
        .checkpoint
        .lifecycle_history
        .iter()
        .filter(|entry| entry.event == "milestone_promoted")
        .filter_map(|entry| {
            entry.metadata.as_ref().and_then(|metadata| {
                metadata
                    .get("milestone_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .collect::<std::collections::HashSet<_>>();
    let completed = completed_mission_milestones(automation, row);
    for milestone in automation_mission_milestones(automation) {
        let milestone_id = milestone
            .get("milestone_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if milestone_id.is_empty()
            || !completed.contains(milestone_id)
            || already_recorded.contains(milestone_id)
        {
            continue;
        }
        let title = milestone
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or(milestone_id);
        let phase_id = milestone
            .get("phase_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let required_stage_ids = milestone
            .get("required_stage_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        record_automation_lifecycle_event_with_metadata(
            row,
            "milestone_promoted",
            Some(format!("milestone `{title}` promoted")),
            None,
            Some(json!({
                "milestone_id": milestone_id,
                "title": title,
                "phase_id": phase_id,
                "required_stage_ids": required_stage_ids,
                "promoted_by_node_id": promoted_by_node_id,
            })),
        );
    }
}

pub fn collect_automation_descendants(
    automation: &AutomationV2Spec,
    root_ids: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut descendants = root_ids.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for node in &automation.flow.nodes {
            if descendants.contains(&node.node_id) {
                continue;
            }
            if node.depends_on.iter().any(|dep| descendants.contains(dep)) {
                descendants.insert(node.node_id.clone());
                changed = true;
            }
        }
    }
    descendants
}

fn render_automation_v2_prompt(
    automation: &AutomationV2Spec,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
    template_system_prompt: Option<&str>,
    standup_report_path: Option<&str>,
    memory_project_id: Option<&str>,
) -> String {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("structured_json");
    let mut sections = Vec::new();
    if let Some(system_prompt) = template_system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Template system prompt:\n{}", system_prompt));
    }
    if let Some(mission) = automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("mission"))
    {
        let mission_title = mission
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(automation.name.as_str());
        let mission_goal = mission
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let success_criteria = mission
            .get("success_criteria")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(|row| format!("- {}", row.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let shared_context = mission
            .get("shared_context")
            .and_then(Value::as_str)
            .unwrap_or_default();
        sections.push(format!(
            "Mission Brief:\nTitle: {mission_title}\nGoal: {mission_goal}\nShared context: {shared_context}\nSuccess criteria:\n{}",
            if success_criteria.is_empty() {
                "- none provided".to_string()
            } else {
                success_criteria
            }
        ));
    }
    sections.push(format!(
        "Automation ID: {}\nRun ID: {}\nNode ID: {}\nAgent: {}\nObjective: {}\nOutput contract kind: {}",
        automation.automation_id, run_id, node.node_id, agent.display_name, node.objective, contract_kind
    ));
    if let Some(contract) = node.output_contract.as_ref() {
        let schema = contract
            .schema
            .as_ref()
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".to_string());
        let guidance = contract.summary_guidance.as_deref().unwrap_or("none");
        sections.push(format!(
            "Output Contract:\nKind: {}\nSummary guidance: {}\nSchema:\n{}",
            contract.kind, guidance, schema
        ));
    }
    if let Some(builder) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
    {
        let local_title = builder
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(node.node_id.as_str());
        let local_prompt = builder
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let local_role = builder
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        sections.push(format!(
            "Local Assignment:\nTitle: {local_title}\nRole: {local_role}\nInstructions: {local_prompt}"
        ));
    }
    let execution_mode = automation_node_execution_mode(node, workspace_root);
    sections.push(format!(
        "Execution Policy:\n- Mode: `{}`.\n- Use only declared workflow artifact paths.\n- Keep status and blocker notes in the response JSON, not as placeholder file contents.",
        execution_mode
    ));
    if automation_node_is_code_workflow(node) {
        let task_kind =
            automation_node_task_kind(node).unwrap_or_else(|| "code_change".to_string());
        let project_backlog_tasks = automation_node_projects_backlog_tasks(node);
        let task_id = automation_node_task_id(node).unwrap_or_else(|| "unassigned".to_string());
        let repo_root = automation_node_repo_root(node).unwrap_or_else(|| ".".to_string());
        let write_scope =
            automation_node_write_scope(node).unwrap_or_else(|| "repo-scoped edits".to_string());
        let acceptance_criteria = automation_node_acceptance_criteria(node)
            .unwrap_or_else(|| "satisfy the declared coding task acceptance criteria".to_string());
        let task_dependencies =
            automation_node_task_dependencies(node).unwrap_or_else(|| "none declared".to_string());
        let verification_state =
            automation_node_verification_state(node).unwrap_or_else(|| "pending".to_string());
        let task_owner =
            automation_node_task_owner(node).unwrap_or_else(|| "unclaimed".to_string());
        let verification_command =
            automation_node_verification_command(node).unwrap_or_else(|| {
                "run the most relevant repo-local build, test, or lint commands".to_string()
            });
        sections.push(format!(
            "Coding Task Context:\n- Task id: `{}`.\n- Task kind: `{}`.\n- Repo root: `{}`.\n- Declared write scope: {}.\n- Acceptance criteria: {}.\n- Backlog dependencies: {}.\n- Verification state: {}.\n- Preferred owner: {}.\n- Verification expectation: {}.\n- Projects backlog tasks: {}.\n- Prefer repository edits plus a concise handoff artifact, not placeholder file rewrites.\n- Use `bash` for verification commands when tool access allows it.",
            task_id, task_kind, repo_root, write_scope, acceptance_criteria, task_dependencies, verification_state, task_owner, verification_command, if project_backlog_tasks { "yes" } else { "no" }
        ));
    }
    if let Some(output_path) = automation_node_required_output_path(node) {
        let output_rules = match execution_mode {
            "git_patch" => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `apply_patch` for multi-line source edits and `edit` for localized replacements.\n- Use `write` only for brand-new files or when patch/edit cannot express the change.\n- Do not replace an existing source file with a status note, preservation note, or placeholder summary.\n- Only write declared workflow artifact files.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
            "filesystem_patch" => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `edit` for existing-file changes.\n- Use `write` for brand-new files or as a last resort when an edit cannot express the change.\n- Do not replace an existing file with a status note, preservation note, or placeholder summary.\n- Only write declared workflow artifact files.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
            _ => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Use the `write` tool to create the full file contents.\n- Only write declared workflow artifact files; do not create auxiliary touch files, status files, marker files, or placeholder preservation notes.\n- Overwrite the declared output with the actual artifact contents for this run instead of preserving a prior placeholder.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
        };
        sections.push(output_rules);
    }
    if automation_node_web_research_expected(node) {
        sections.push(
            "External Research Expectation:\n- Use `websearch` for current external evidence before finalizing the output file.\n- Include only evidence you can support from local files or current web findings."
                .to_string(),
        );
    }
    let mut prompt = sections.join("\n\n");
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
    if let Some(report_path) = standup_report_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nStandup report path:\n- Write the final markdown report to `{}` relative to the workspace root.\n- Use the `write` tool for the report.\n- The report must remain inside the workspace.",
            report_path
        ));
    }
    if let Some(project_id) = memory_project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nMemory search scope:\n- `memory_search` defaults to the current session, current project, and global memory.\n- Current project_id: `{}`.\n- Use `tier: \"project\"` when you need recall limited to this workspace.\n- Use workspace files via `glob`, `grep`, and `read` when memory is sparse or stale.",
            project_id
        ));
    }
    let validator_kind = automation_output_validator_kind(node);
    let enforce_completed_first_attempt = (validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || !automation_node_required_tools(node).is_empty())
        && attempt <= 1;
    if enforce_completed_first_attempt {
        prompt.push_str(
            "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the write tool actually created the output file.",
        );
    } else {
        prompt.push_str(
            "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim semantic success if the output is blocked or not approved.",
        );
    }
    prompt
}

fn render_automation_repair_brief(
    node: &AutomationFlowNode,
    prior_output: Option<&Value>,
    attempt: u32,
    max_attempts: u32,
) -> Option<String> {
    if attempt <= 1 {
        return None;
    }
    let prior_output = prior_output?;
    if !automation_output_needs_repair(prior_output) {
        return None;
    }

    let validator_summary = prior_output.get("validator_summary");
    let artifact_validation = prior_output.get("artifact_validation");
    let tool_telemetry = prior_output.get("tool_telemetry");
    let reason = validator_summary
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("the previous attempt did not satisfy the runtime validator");
    let unmet_requirements = validator_summary
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let blocking_classification = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unspecified");
    let required_next_tool_actions = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tools_offered = tool_telemetry
        .and_then(|value| value.get("requested_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tools_executed = tool_telemetry
        .and_then(|value| value.get("executed_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unreviewed_relevant_paths = artifact_validation
        .and_then(|value| value.get("unreviewed_relevant_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(attempt.saturating_sub(1));
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| max_attempts.saturating_sub(attempt.saturating_sub(1)));

    let unmet_line = if unmet_requirements.is_empty() {
        "none recorded".to_string()
    } else {
        unmet_requirements.join(", ")
    };
    let tools_offered_line = if tools_offered.is_empty() {
        "none recorded".to_string()
    } else {
        tools_offered.join(", ")
    };
    let tools_executed_line = if tools_executed.is_empty() {
        "none recorded".to_string()
    } else {
        tools_executed.join(", ")
    };
    let unreviewed_line = if unreviewed_relevant_paths.is_empty() {
        "none recorded".to_string()
    } else {
        unreviewed_relevant_paths.join(", ")
    };
    let next_actions_line = if required_next_tool_actions.is_empty() {
        "none recorded".to_string()
    } else {
        required_next_tool_actions.join(" | ")
    };

    Some(format!(
        "Repair Brief:\n- Node `{}` is being retried because the previous attempt ended in `needs_repair`.\n- Previous validation reason: {}.\n- Unmet requirements: {}.\n- Blocking classification: {}.\n- Required next tool actions: {}.\n- Tools offered last attempt: {}.\n- Tools executed last attempt: {}.\n- Relevant files still unread or explicitly unreviewed: {}.\n- Previous repair attempt count: {}.\n- Remaining repair attempts after this run: {}.\n- For this retry, satisfy the unmet requirements before finalizing the artifact.\n- Do not write a blocked handoff unless the required tools were actually attempted and remained unavailable or failed.",
        node.node_id,
        reason,
        unmet_line,
        blocking_classification,
        next_actions_line,
        tools_offered_line,
        tools_executed_line,
        unreviewed_line,
        repair_attempt,
        repair_attempts_remaining.saturating_sub(1),
    ))
}

fn is_agent_standup_automation(automation: &AutomationV2Spec) -> bool {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("feature"))
        .and_then(Value::as_str)
        .map(|value| value == "agent_standup")
        .unwrap_or(false)
}

fn resolve_standup_report_path_template(automation: &AutomationV2Spec) -> Option<String> {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("standup"))
        .and_then(|value| value.get("report_path_template"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_standup_report_path_for_run(
    automation: &AutomationV2Spec,
    started_at_ms: u64,
) -> Option<String> {
    let template = resolve_standup_report_path_template(automation)?;
    if !template.contains("{{date}}") {
        return Some(template);
    }
    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(started_at_ms as i64)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m-%d")
        .to_string();
    Some(template.replace("{{date}}", &date))
}

fn automation_workspace_project_id(workspace_root: &str) -> String {
    tandem_core::workspace_project_id(workspace_root)
        .unwrap_or_else(|| "workspace-unknown".to_string())
}

fn merge_automation_agent_allowlist(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Vec<String> {
    let mut allowlist = if agent.tool_policy.allowlist.is_empty() {
        template
            .map(|value| value.capabilities.tool_allowlist.clone())
            .unwrap_or_default()
    } else {
        agent.tool_policy.allowlist.clone()
    };
    allowlist.sort();
    allowlist.dedup();
    allowlist
}

fn automation_node_output_extension(node: &AutomationFlowNode) -> Option<String> {
    automation_node_required_output_path(node)
        .as_deref()
        .and_then(|value| std::path::Path::new(value).extension())
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

fn automation_node_task_kind(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_kind")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn automation_node_projects_backlog_tasks(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("project_backlog_tasks"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_task_id(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_repo_root(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "repo_root")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_write_scope(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "write_scope")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_acceptance_criteria(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "acceptance_criteria")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_task_dependencies(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_dependencies")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_verification_state(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "verification_state")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_task_owner(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_owner")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_verification_command(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "verification_command")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Clone, Debug)]
struct AutomationVerificationStep {
    kind: String,
    command: String,
}

fn infer_verification_kind(command: &str) -> String {
    let lowered = command.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return "verify".to_string();
    }
    if lowered.starts_with("build:")
        || lowered.contains(" cargo build")
        || lowered.starts_with("cargo build")
        || lowered.contains(" npm run build")
        || lowered.starts_with("npm run build")
        || lowered.contains(" pnpm build")
        || lowered.starts_with("pnpm build")
        || lowered.contains(" yarn build")
        || lowered.starts_with("yarn build")
        || lowered.contains(" tsc")
        || lowered.starts_with("tsc")
        || lowered.starts_with("cargo check")
        || lowered.contains(" cargo check")
    {
        return "build".to_string();
    }
    if lowered.starts_with("test:")
        || lowered.contains(" cargo test")
        || lowered.starts_with("cargo test")
        || lowered.contains(" pytest")
        || lowered.starts_with("pytest")
        || lowered.contains(" npm test")
        || lowered.starts_with("npm test")
        || lowered.contains(" pnpm test")
        || lowered.starts_with("pnpm test")
        || lowered.contains(" yarn test")
        || lowered.starts_with("yarn test")
        || lowered.contains(" go test")
        || lowered.starts_with("go test")
    {
        return "test".to_string();
    }
    if lowered.starts_with("lint:")
        || lowered.contains(" clippy")
        || lowered.starts_with("cargo clippy")
        || lowered.contains(" eslint")
        || lowered.starts_with("eslint")
        || lowered.contains(" ruff")
        || lowered.starts_with("ruff")
        || lowered.contains(" shellcheck")
        || lowered.starts_with("shellcheck")
        || lowered.contains(" fmt --check")
        || lowered.contains(" format")
        || lowered.contains(" lint")
    {
        return "lint".to_string();
    }
    "verify".to_string()
}

fn split_verification_commands(raw: &str) -> Vec<String> {
    let mut commands = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        for chunk in trimmed.split("&&") {
            for piece in chunk.split(';') {
                let candidate = piece.trim();
                if candidate.is_empty() {
                    continue;
                }
                commands.push(candidate.to_string());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    commands
        .into_iter()
        .filter(|value| seen.insert(value.to_ascii_lowercase()))
        .collect()
}

fn automation_node_verification_plan(node: &AutomationFlowNode) -> Vec<AutomationVerificationStep> {
    if let Some(items) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("verification_plan"))
        .and_then(Value::as_array)
    {
        let mut plan = Vec::new();
        for item in items {
            let (kind, command) = if let Some(obj) = item.as_object() {
                let command = obj
                    .get("command")
                    .or_else(|| obj.get("value"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let kind = obj
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_ascii_lowercase);
                (kind, command)
            } else {
                (
                    None,
                    item.as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                )
            };
            let Some(command) = command else {
                continue;
            };
            plan.push(AutomationVerificationStep {
                kind: kind.unwrap_or_else(|| infer_verification_kind(&command)),
                command,
            });
        }
        if !plan.is_empty() {
            return plan;
        }
    }
    automation_node_verification_command(node)
        .map(|raw| {
            split_verification_commands(&raw)
                .into_iter()
                .map(|command| AutomationVerificationStep {
                    kind: infer_verification_kind(&command),
                    command,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn automation_node_is_code_workflow(node: &AutomationFlowNode) -> bool {
    if automation_node_task_kind(node)
        .as_deref()
        .is_some_and(|kind| matches!(kind, "code_change" | "repo_fix" | "implementation"))
    {
        return true;
    }
    let Some(extension) = automation_node_output_extension(node) else {
        return false;
    };
    let code_extensions = [
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "kts", "c", "cc", "cpp", "h",
        "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "zsh",
    ];
    code_extensions.contains(&extension.as_str())
}

fn automation_output_validator_kind(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputValidatorKind {
    if let Some(validator) = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.validator)
    {
        return validator;
    }
    if automation_node_is_code_workflow(node) {
        return crate::AutomationOutputValidatorKind::CodePatch;
    }
    match node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("brief") => crate::AutomationOutputValidatorKind::ResearchBrief,
        Some("review") => crate::AutomationOutputValidatorKind::ReviewDecision,
        Some("structured_json") => crate::AutomationOutputValidatorKind::StructuredJson,
        _ => crate::AutomationOutputValidatorKind::GenericArtifact,
    }
}

fn path_looks_like_source_file(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.replace('\\', "/");
    let path = std::path::Path::new(&normalized);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    if extension.as_deref().is_some_and(|extension| {
        [
            "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "kts", "c", "cc", "cpp", "h",
            "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "zsh", "toml", "yaml", "yml",
            "json",
        ]
        .contains(&extension)
    }) {
        return true;
    }
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|name| {
            matches!(
                name.as_str(),
                "cargo.toml"
                    | "cargo.lock"
                    | "package.json"
                    | "package-lock.json"
                    | "pnpm-lock.yaml"
                    | "tsconfig.json"
                    | "deno.json"
                    | "deno.jsonc"
                    | "jest.config.js"
                    | "jest.config.ts"
                    | "vite.config.ts"
                    | "vite.config.js"
                    | "webpack.config.js"
                    | "webpack.config.ts"
                    | "next.config.js"
                    | "next.config.mjs"
                    | "pyproject.toml"
                    | "requirements.txt"
                    | "makefile"
                    | "dockerfile"
            )
        })
}

fn workspace_has_git_repo(workspace_root: &str) -> bool {
    std::process::Command::new("git")
        .current_dir(workspace_root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn automation_node_execution_mode(node: &AutomationFlowNode, workspace_root: &str) -> &'static str {
    if !automation_node_is_code_workflow(node) {
        return "artifact_write";
    }
    if workspace_has_git_repo(workspace_root) {
        "git_patch"
    } else {
        "filesystem_patch"
    }
}

fn normalize_automation_requested_tools(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
) -> Vec<String> {
    let mut normalized = config::channels::normalize_allowed_tools(raw);
    if normalized.iter().any(|tool| tool == "*") {
        return vec!["*".to_string()];
    }
    match automation_node_execution_mode(node, workspace_root) {
        "git_patch" => {
            normalized.extend([
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "apply_patch".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ]);
        }
        "filesystem_patch" => {
            normalized.extend([
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ]);
        }
        _ => {
            if automation_node_required_output_path(node).is_some() {
                normalized.push("write".to_string());
            }
        }
    }
    let has_read = normalized.iter().any(|tool| tool == "read");
    let has_workspace_probe = normalized
        .iter()
        .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list"));
    if has_read && !has_workspace_probe {
        normalized.push("glob".to_string());
    }
    if automation_node_web_research_expected(node) {
        normalized.push("websearch".to_string());
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

fn automation_node_prewrite_requirements(
    node: &AutomationFlowNode,
    requested_tools: &[String],
) -> Option<PrewriteRequirements> {
    let write_required = automation_node_required_output_path(node).is_some();
    if !write_required {
        return None;
    }
    let workspace_inspection_required = requested_tools
        .iter()
        .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list" | "read"));
    let web_research_required = automation_node_web_research_expected(node)
        && requested_tools.iter().any(|tool| tool == "websearch");
    let brief_research_node = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief;
    let required_tools = automation_node_required_tools(node);
    let has_required_read = required_tools.iter().any(|tool| tool == "read");
    let has_required_websearch = required_tools.iter().any(|tool| tool == "websearch");
    let has_any_required_tools = !required_tools.is_empty();
    let concrete_read_required = (brief_research_node || has_required_read)
        && requested_tools.iter().any(|tool| tool == "read");
    let successful_web_research_required = (brief_research_node || has_required_websearch)
        && automation_node_web_research_expected(node)
        && requested_tools.iter().any(|tool| tool == "websearch");
    Some(PrewriteRequirements {
        workspace_inspection_required,
        web_research_required,
        concrete_read_required,
        successful_web_research_required,
        repair_on_unmet_requirements: brief_research_node || has_any_required_tools,
        coverage_mode: if brief_research_node {
            PrewriteCoverageMode::ResearchCorpus
        } else {
            PrewriteCoverageMode::None
        },
    })
}

fn resolve_automation_agent_model(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Option<ModelSpec> {
    if let Some(model) = agent
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(crate::app::routines::parse_model_spec)
    {
        return Some(model);
    }
    template
        .and_then(|value| value.default_model.as_ref())
        .and_then(crate::app::routines::parse_model_spec)
}

pub fn automation_node_required_output_path(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("output_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn automation_node_web_research_expected(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("web_research_expected"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_required_tools(node: &AutomationFlowNode) -> Vec<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("required_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn automation_node_execution_policy(node: &AutomationFlowNode, workspace_root: &str) -> Value {
    let output_path = automation_node_required_output_path(node);
    let code_workflow = automation_node_is_code_workflow(node);
    let git_backed = workspace_has_git_repo(workspace_root);
    let mode = automation_node_execution_mode(node, workspace_root);
    let workflow_class = automation_node_workflow_class(node);
    json!({
        "mode": mode,
        "workflow_class": workflow_class,
        "code_workflow": code_workflow,
        "git_backed": git_backed,
        "declared_output_path": output_path,
        "project_backlog_tasks": automation_node_projects_backlog_tasks(node),
        "task_id": automation_node_task_id(node),
        "task_kind": automation_node_task_kind(node),
        "repo_root": automation_node_repo_root(node),
        "write_scope": automation_node_write_scope(node),
        "acceptance_criteria": automation_node_acceptance_criteria(node),
        "task_dependencies": automation_node_task_dependencies(node),
        "verification_state": automation_node_verification_state(node),
        "task_owner": automation_node_task_owner(node),
        "verification_command": automation_node_verification_command(node),
    })
}

fn resolve_automation_output_path(
    workspace_root: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let trimmed = output_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("required output path is empty");
    }
    let workspace = PathBuf::from(workspace_root);
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    if !resolved.starts_with(&workspace) {
        anyhow::bail!(
            "required output path `{}` must stay inside workspace `{}`",
            trimmed,
            workspace_root
        );
    }
    Ok(resolved)
}

fn is_suspicious_automation_marker_file(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lowered = name.to_ascii_lowercase();
    lowered.starts_with(".tandem")
        || lowered == "_automation_touch.txt"
        || lowered.contains("stage-touch")
        || lowered.ends_with("-status.txt")
        || lowered.contains("touch.txt")
}

fn list_suspicious_automation_marker_files(workspace_root: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_suspicious_automation_marker_file(path))
        .filter_map(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn remove_suspicious_automation_marker_files(workspace_root: &str) {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_suspicious_automation_marker_file(&path) {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

fn automation_workspace_root_file_snapshot(
    workspace_root: &str,
) -> std::collections::BTreeSet<String> {
    let workspace = PathBuf::from(workspace_root);
    let mut snapshot = std::collections::BTreeSet::new();
    let mut stack = vec![workspace.clone()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let display = path
                .strip_prefix(&workspace)
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            snapshot.insert(display);
        }
    }
    snapshot
}

fn placeholder_like_artifact_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    // TODO(coding-hardening): Replace this phrase-based placeholder detection with
    // structural artifact validation. The long-term design should score artifact
    // substance from session mutation history + contract-kind-specific structure
    // (sections, length, citations, required headings) rather than hard-coded text
    // markers that are brittle across providers, prompts, and languages.
    if trimmed.len() <= 160 {
        let compact = trimmed.to_ascii_lowercase();
        let status_only_markers = [
            "completed",
            "written to",
            "already written",
            "no content change",
            "no content changes",
            "confirmed",
            "preserving existing artifact",
            "finalization",
            "write completion",
        ];
        if status_only_markers
            .iter()
            .any(|marker| compact.contains(marker))
            && !compact.contains("## ")
            && !compact.contains("\n## ")
            && !compact.contains("files reviewed")
            && !compact.contains("proof points")
        {
            return true;
        }
    }
    let lowered = trimmed
        .chars()
        .take(800)
        .collect::<String>()
        .to_ascii_lowercase();
    let strong_markers = [
        "completed previously in this run",
        "preserving file creation requirement",
        "preserving current workspace output state",
        "created/updated to satisfy workflow artifact requirement",
        "see existing workspace research already completed in this run",
        "already written in prior step",
        "no content changes needed",
        "placeholder preservation note",
        "touch file",
        "status note",
        "marker file",
    ];
    if strong_markers.iter().any(|marker| lowered.contains(marker)) {
        return true;
    }
    let status_markers = [
        "# status",
        "## status",
        "status: blocked",
        "status: completed",
        "status: pending",
        "blocked handoff",
        "blocked note",
        "not approved yet",
        "pending approval",
    ];
    status_markers.iter().any(|marker| lowered.contains(marker)) && trimmed.len() < 280
}

fn markdown_heading_count(text: &str) -> usize {
    text.lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .count()
}

fn markdown_list_item_count(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit() && trimmed.contains('.'))
        })
        .count()
}

fn paragraph_block_count(text: &str) -> usize {
    text.split("\n\n")
        .filter(|block| {
            let trimmed = block.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .count()
}

fn structural_substantive_artifact_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 180 {
        return false;
    }
    let heading_count = markdown_heading_count(trimmed);
    let list_count = markdown_list_item_count(trimmed);
    let paragraph_count = paragraph_block_count(trimmed);
    heading_count >= 2
        || (heading_count >= 1 && paragraph_count >= 3)
        || (paragraph_count >= 4)
        || (list_count >= 5)
}

fn substantive_artifact_text(text: &str) -> bool {
    structural_substantive_artifact_text(text)
}

#[derive(Debug, Clone)]
struct ArtifactCandidateAssessment {
    source: String,
    text: String,
    length: usize,
    score: i64,
    substantive: bool,
    placeholder_like: bool,
    heading_count: usize,
    list_count: usize,
    paragraph_count: usize,
    required_section_count: usize,
    files_reviewed_present: bool,
    reviewed_paths: Vec<String>,
    reviewed_paths_backed_by_read: Vec<String>,
    unreviewed_relevant_paths: Vec<String>,
    citation_count: usize,
    web_sources_reviewed_present: bool,
}

fn artifact_required_section_count(node: &AutomationFlowNode, text: &str) -> usize {
    let lowered = text.to_ascii_lowercase();
    let headings = if automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
    {
        vec![
            "workspace source audit",
            "campaign goal",
            "target audience",
            "core pain points",
            "positioning angle",
            "competitor context",
            "proof points",
            "likely objections",
            "channel considerations",
            "recommended message hierarchy",
            "files reviewed",
            "files not reviewed",
            "web sources reviewed",
        ]
    } else {
        vec![
            "files reviewed",
            "review notes",
            "approved",
            "draft",
            "summary",
        ]
    };
    headings
        .iter()
        .filter(|heading| lowered.contains(**heading))
        .count()
}

fn artifact_candidate_source_priority(source: &str) -> i64 {
    match source {
        "verified_output" => 3,
        "session_write" => 2,
        "preexisting_output" => 1,
        _ => 0,
    }
}

fn assess_artifact_candidate(
    node: &AutomationFlowNode,
    workspace_root: &str,
    source: &str,
    text: &str,
    read_paths: &[String],
    discovered_relevant_paths: &[String],
) -> ArtifactCandidateAssessment {
    let trimmed = text.trim();
    let length = trimmed.len();
    let placeholder_like = placeholder_like_artifact_text(trimmed);
    let substantive = substantive_artifact_text(trimmed);
    let heading_count = markdown_heading_count(trimmed);
    let list_count = markdown_list_item_count(trimmed);
    let paragraph_count = paragraph_block_count(trimmed);
    let required_section_count = artifact_required_section_count(node, trimmed);
    let reviewed_paths = extract_markdown_section_paths(trimmed, "Files reviewed")
        .into_iter()
        .filter_map(|value| normalize_workspace_display_path(workspace_root, &value))
        .collect::<Vec<_>>();
    let files_not_reviewed = extract_markdown_section_paths(trimmed, "Files not reviewed")
        .into_iter()
        .filter_map(|value| normalize_workspace_display_path(workspace_root, &value))
        .collect::<Vec<_>>();
    let reviewed_paths_backed_by_read = reviewed_paths
        .iter()
        .filter(|path| read_paths.iter().any(|read| read == *path))
        .cloned()
        .collect::<Vec<_>>();
    let files_reviewed_present = files_reviewed_section_lists_paths(trimmed);
    let citation_count = markdown_citation_count(trimmed);
    let web_sources_reviewed_present = web_sources_reviewed_section_lists_sources(trimmed);
    let effective_relevant_paths = if discovered_relevant_paths.is_empty() {
        reviewed_paths.clone()
    } else {
        discovered_relevant_paths.to_vec()
    };
    let unreviewed_relevant_paths = effective_relevant_paths
        .iter()
        .filter(|path| {
            !read_paths.iter().any(|read| read == *path)
                && !files_not_reviewed.iter().any(|skipped| skipped == *path)
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut score = 0i64;
    score += artifact_candidate_source_priority(source) * 25;
    score += (length.min(12_000) / 24) as i64;
    score += (heading_count as i64) * 60;
    score += (list_count as i64) * 18;
    score += (paragraph_count as i64) * 24;
    score += (required_section_count as i64) * 160;
    if substantive {
        score += 2_000;
    }
    if files_reviewed_present {
        score += 180;
    }
    score += (citation_count.min(8) as i64) * 45;
    if web_sources_reviewed_present {
        score += 140;
    }
    if !reviewed_paths.is_empty() && reviewed_paths.len() == reviewed_paths_backed_by_read.len() {
        score += 260;
    } else if !reviewed_paths_backed_by_read.is_empty() {
        score += 90;
    }
    score -= (unreviewed_relevant_paths.len() as i64) * 220;
    if placeholder_like {
        score -= 450;
    }
    if trimmed.is_empty() {
        score -= 2_000;
    }

    ArtifactCandidateAssessment {
        source: source.to_string(),
        text: text.to_string(),
        length,
        score,
        substantive,
        placeholder_like,
        heading_count,
        list_count,
        paragraph_count,
        required_section_count,
        files_reviewed_present,
        reviewed_paths,
        reviewed_paths_backed_by_read,
        unreviewed_relevant_paths,
        citation_count,
        web_sources_reviewed_present,
    }
}

fn best_artifact_candidate(
    candidates: &[ArtifactCandidateAssessment],
) -> Option<ArtifactCandidateAssessment> {
    candidates.iter().cloned().max_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then(left.substantive.cmp(&right.substantive))
            .then(
                left.required_section_count
                    .cmp(&right.required_section_count),
            )
            .then(left.heading_count.cmp(&right.heading_count))
            .then(left.length.cmp(&right.length))
            .then(
                artifact_candidate_source_priority(&left.source)
                    .cmp(&artifact_candidate_source_priority(&right.source)),
            )
    })
}

fn files_reviewed_section_lists_paths(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let Some(start) = lowered.find("files reviewed") else {
        return false;
    };
    let tail = &text[start..];
    tail.lines().skip(1).take(24).any(|line| {
        let trimmed = line.trim();
        (trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            && (trimmed.contains('/')
                || trimmed.contains(".md")
                || trimmed.contains(".txt")
                || trimmed.contains("readme"))
    })
}

fn markdown_citation_count(text: &str) -> usize {
    let markdown_links = text.match_indices("](").count();
    let bare_urls = text
        .split_whitespace()
        .filter(|token| {
            let trimmed = token.trim_matches(|ch: char| {
                matches!(ch, ')' | '(' | '[' | ']' | ',' | '.' | ';' | '"' | '\'')
            });
            trimmed.starts_with("http://") || trimmed.starts_with("https://")
        })
        .count();
    markdown_links.max(bare_urls)
}

fn web_sources_reviewed_section_lists_sources(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let Some(start) = lowered.find("web sources reviewed") else {
        return false;
    };
    let tail = &text[start..];
    tail.lines().skip(1).take(24).any(|line| {
        let trimmed = line.trim();
        (trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            && (trimmed.contains("http://")
                || trimmed.contains("https://")
                || trimmed.contains("]("))
    })
}

fn extract_markdown_section_paths(text: &str, heading: &str) -> Vec<String> {
    let mut collecting = false;
    let mut paths = Vec::new();
    let heading_normalized = heading.trim().to_ascii_lowercase();
    for line in text.lines() {
        let trimmed = line.trim();
        let normalized = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
        if trimmed.starts_with('#') {
            collecting = normalized == heading_normalized;
            continue;
        }
        if !collecting {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let candidate = trimmed
            .trim_start_matches(|ch: char| {
                ch == '-' || ch == '*' || ch.is_ascii_digit() || ch == '.' || ch == ')'
            })
            .trim();
        let token = candidate.split(['`', '(', ')']).find_map(|part| {
            let value = part.trim();
            if value.contains('/')
                || value.ends_with(".md")
                || value.ends_with(".txt")
                || value.to_ascii_lowercase().contains("readme")
            {
                Some(value.to_string())
            } else {
                None
            }
        });
        if let Some(path) = token.filter(|value| !value.is_empty()) {
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn normalize_workspace_display_path(workspace_root: &str, raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim().trim_matches('`');
    if trimmed.is_empty() {
        return None;
    }
    resolve_automation_output_path(workspace_root, trimmed)
        .ok()
        .and_then(|resolved| {
            resolved
                .strip_prefix(PathBuf::from(workspace_root))
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
        })
        .filter(|value| !value.is_empty())
}

fn tool_args_object(args: &Value) -> Option<std::borrow::Cow<'_, serde_json::Map<String, Value>>> {
    match args {
        Value::Object(map) => Some(std::borrow::Cow::Borrowed(map)),
        Value::String(raw) => {
            serde_json::from_str::<Value>(raw)
                .ok()
                .and_then(|value| match value {
                    Value::Object(map) => Some(std::borrow::Cow::Owned(map)),
                    _ => None,
                })
        }
        _ => None,
    }
}

fn session_read_paths(session: &Session, workspace_root: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("read")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str) else {
                continue;
            };
            if let Some(normalized) = normalize_workspace_display_path(workspace_root, path) {
                paths.push(normalized);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn session_discovered_relevant_paths(session: &Session, workspace_root: &str) -> Vec<String> {
    let workspace = PathBuf::from(workspace_root);
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                result,
                error,
                ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("glob")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let output = result
                .as_ref()
                .and_then(|value| value.get("output"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            for line in output.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let path = PathBuf::from(trimmed);
                let resolved = if path.is_absolute() {
                    path
                } else {
                    let Ok(resolved) = resolve_automation_output_path(workspace_root, trimmed)
                    else {
                        continue;
                    };
                    resolved
                };
                if !resolved.starts_with(&workspace) {
                    continue;
                }
                if !std::fs::metadata(&resolved)
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    continue;
                }
                let display = resolved
                    .strip_prefix(&workspace)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty());
                if let Some(display) = display {
                    paths.push(display);
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn session_write_candidates_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
) -> Vec<String> {
    let Ok(target_path) = resolve_automation_output_path(workspace_root, declared_output_path)
    else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let Ok(candidate_path) = resolve_automation_output_path(workspace_root, path) else {
                continue;
            };
            if candidate_path != target_path {
                continue;
            }
            let Some(content) = args.get("content").and_then(Value::as_str) else {
                continue;
            };
            if !content.trim().is_empty() {
                candidates.push(content.to_string());
            }
        }
    }
    candidates
}

fn artifact_candidate_summary(candidate: &ArtifactCandidateAssessment, accepted: bool) -> Value {
    json!({
        "source": candidate.source,
        "length": candidate.length,
        "score": candidate.score,
        "substantive": candidate.substantive,
        "placeholder_like": candidate.placeholder_like,
        "heading_count": candidate.heading_count,
        "list_count": candidate.list_count,
        "paragraph_count": candidate.paragraph_count,
        "required_section_count": candidate.required_section_count,
        "files_reviewed_present": candidate.files_reviewed_present,
        "reviewed_paths_backed_by_read": candidate.reviewed_paths_backed_by_read,
        "unreviewed_relevant_paths": candidate.unreviewed_relevant_paths,
        "citation_count": candidate.citation_count,
        "web_sources_reviewed_present": candidate.web_sources_reviewed_present,
        "accepted": accepted,
    })
}

fn session_file_mutation_summary(session: &Session, workspace_root: &str) -> Value {
    let mut touched_files = Vec::<String>::new();
    let mut mutation_tool_by_file = serde_json::Map::new();
    let workspace_root_path = PathBuf::from(workspace_root);
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                continue;
            }
            let tool_name = tool.trim().to_ascii_lowercase().replace('-', "_");
            let parsed_args = tool_args_object(args);
            let candidate_paths = if tool_name == "apply_patch" {
                parsed_args
                    .as_ref()
                    .and_then(|args| args.get("patchText"))
                    .and_then(Value::as_str)
                    .map(|patch| {
                        patch
                            .lines()
                            .filter_map(|line| {
                                let trimmed = line.trim();
                                trimmed
                                    .strip_prefix("*** Add File: ")
                                    .or_else(|| trimmed.strip_prefix("*** Update File: "))
                                    .or_else(|| trimmed.strip_prefix("*** Delete File: "))
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                    .map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                parsed_args
                    .as_ref()
                    .and_then(|args| args.get("path"))
                    .and_then(Value::as_str)
                    .map(|value| vec![value.trim().to_string()])
                    .unwrap_or_default()
            };
            for candidate in candidate_paths {
                let Some(resolved) = resolve_automation_output_path(workspace_root, &candidate)
                    .ok()
                    .or_else(|| {
                        let path = PathBuf::from(candidate.trim());
                        if path.is_absolute()
                            && tandem_core::is_within_workspace_root(&path, &workspace_root_path)
                        {
                            Some(path)
                        } else {
                            None
                        }
                    })
                else {
                    continue;
                };
                let display = resolved
                    .strip_prefix(&workspace_root_path)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| resolved.to_string_lossy().to_string());
                if !touched_files.iter().any(|existing| existing == &display) {
                    touched_files.push(display.clone());
                }
                match mutation_tool_by_file.get_mut(&display) {
                    Some(Value::Array(values)) => {
                        if !values
                            .iter()
                            .any(|value| value.as_str() == Some(tool_name.as_str()))
                        {
                            values.push(json!(tool_name.clone()));
                        }
                    }
                    _ => {
                        mutation_tool_by_file.insert(display.clone(), json!([tool_name.clone()]));
                    }
                }
            }
        }
    }
    touched_files.sort();
    json!({
        "touched_files": touched_files,
        "mutation_tool_by_file": mutation_tool_by_file,
    })
}

fn session_verification_summary(node: &AutomationFlowNode, session: &Session) -> Value {
    let verification_plan = automation_node_verification_plan(node);
    let Some(expected_command) = automation_node_verification_command(node) else {
        return json!({
            "verification_expected": false,
            "verification_command": Value::Null,
            "verification_plan": [],
            "verification_results": [],
            "verification_outcome": Value::Null,
            "verification_total": 0,
            "verification_completed": 0,
            "verification_passed_count": 0,
            "verification_failed_count": 0,
            "verification_ran": false,
            "verification_failed": false,
            "latest_verification_command": Value::Null,
            "latest_verification_failure": Value::Null,
        });
    };
    let verification_plan = if verification_plan.is_empty() {
        vec![AutomationVerificationStep {
            kind: infer_verification_kind(&expected_command),
            command: expected_command.clone(),
        }]
    } else {
        verification_plan
    };
    let mut verification_results = verification_plan
        .iter()
        .map(|step| {
            json!({
                "kind": step.kind,
                "command": step.command,
                "ran": false,
                "failed": false,
                "failure": Value::Null,
                "latest_command": Value::Null,
            })
        })
        .collect::<Vec<_>>();
    let mut verification_ran = false;
    let mut verification_failed = false;
    let mut latest_verification_command = None::<String>;
    let mut latest_verification_failure = None::<String>;
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } = part
            else {
                continue;
            };
            if tool.trim().to_ascii_lowercase().replace('-', "_") != "bash" {
                continue;
            }
            let Some(command) = args.get("command").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let command_normalized = command.to_ascii_lowercase();
            let failure = if let Some(error) = error
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(error.to_string())
            } else {
                let metadata = result
                    .as_ref()
                    .and_then(|value| value.get("metadata"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let exit_code = metadata.get("exit_code").and_then(Value::as_i64);
                let timed_out = metadata
                    .get("timeout")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let cancelled = metadata
                    .get("cancelled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let stderr = metadata
                    .get("stderr")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                if timed_out {
                    Some(format!("verification command timed out: {}", command))
                } else if cancelled {
                    Some(format!("verification command was cancelled: {}", command))
                } else if exit_code.is_some_and(|code| code != 0) {
                    Some(
                        stderr
                            .filter(|value| !value.is_empty())
                            .map(|value| {
                                format!(
                                    "verification command failed with exit code {}: {}",
                                    exit_code.unwrap_or_default(),
                                    truncate_text(&value, 240)
                                )
                            })
                            .unwrap_or_else(|| {
                                format!(
                                    "verification command failed with exit code {}: {}",
                                    exit_code.unwrap_or_default(),
                                    command
                                )
                            }),
                    )
                } else {
                    None
                }
            };
            for result in &mut verification_results {
                let Some(expected) = result.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let expected_normalized = expected.trim().to_ascii_lowercase();
                if !command_normalized.contains(&expected_normalized) {
                    continue;
                }
                verification_ran = true;
                latest_verification_command = Some(command.to_string());
                if let Some(object) = result.as_object_mut() {
                    object.insert("ran".to_string(), json!(true));
                    object.insert("latest_command".to_string(), json!(command.to_string()));
                    if let Some(failure_text) = failure.clone() {
                        verification_failed = true;
                        latest_verification_failure = Some(failure_text.clone());
                        object.insert("failed".to_string(), json!(true));
                        object.insert("failure".to_string(), json!(failure_text));
                    }
                }
            }
        }
    }
    let verification_completed = verification_results
        .iter()
        .filter(|value| value.get("ran").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let verification_failed_count = verification_results
        .iter()
        .filter(|value| {
            value
                .get("failed")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let verification_passed_count = verification_results
        .iter()
        .filter(|value| {
            value.get("ran").and_then(Value::as_bool).unwrap_or(false)
                && !value
                    .get("failed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .count();
    let verification_total = verification_results.len();
    let verification_outcome = if verification_total == 0 {
        None
    } else if verification_failed_count > 0 {
        Some("failed")
    } else if verification_completed == 0 {
        Some("missing")
    } else if verification_completed < verification_total {
        Some("partial")
    } else {
        Some("passed")
    };
    json!({
        "verification_expected": true,
        "verification_command": expected_command,
        "verification_plan": verification_plan
            .iter()
            .map(|step| json!({"kind": step.kind, "command": step.command}))
            .collect::<Vec<_>>(),
        "verification_results": verification_results,
        "verification_outcome": verification_outcome,
        "verification_total": verification_total,
        "verification_completed": verification_completed,
        "verification_passed_count": verification_passed_count,
        "verification_failed_count": verification_failed_count,
        "verification_ran": verification_ran,
        "verification_failed": verification_failed,
        "latest_verification_command": latest_verification_command,
        "latest_verification_failure": latest_verification_failure,
    })
}

fn git_diff_summary_for_paths(workspace_root: &str, paths: &[String]) -> Option<Value> {
    if paths.is_empty() || !workspace_has_git_repo(workspace_root) {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(workspace_root)
        .arg("diff")
        .arg("--stat")
        .arg("--");
    for path in paths {
        cmd.arg(path);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if summary.is_empty() {
        None
    } else {
        Some(json!({
            "stat": summary
        }))
    }
}

fn validate_automation_artifact_output(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
) -> (Option<(String, String)>, Value, Option<String>) {
    let suspicious_after = list_suspicious_automation_marker_files(workspace_root);
    let undeclared_files_created = suspicious_after
        .iter()
        .filter(|name| !workspace_snapshot_before.contains((*name).as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut auto_cleaned = false;
    if !suspicious_after.is_empty() {
        remove_suspicious_automation_marker_files(workspace_root);
        auto_cleaned = true;
    }

    let execution_policy = automation_node_execution_policy(node, workspace_root);
    let mutation_summary = session_file_mutation_summary(session, workspace_root);
    let verification_summary = session_verification_summary(node, session);
    let touched_files = mutation_summary
        .get("touched_files")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mutation_tool_by_file = mutation_summary
        .get("mutation_tool_by_file")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut rejected_reason = if undeclared_files_created.is_empty() {
        None
    } else {
        Some(format!(
            "undeclared marker files created: {}",
            undeclared_files_created.join(", ")
        ))
    };
    let mut semantic_block_reason = None::<String>;
    let mut accepted_output = verified_output;
    let mut recovered_from_session_write = false;
    let read_paths = session_read_paths(session, workspace_root);
    let mut discovered_relevant_paths = session_discovered_relevant_paths(session, workspace_root);
    let mut reviewed_paths_backed_by_read = Vec::<String>::new();
    let mut unreviewed_relevant_paths = Vec::<String>::new();
    let mut unmet_requirements = Vec::<String>::new();
    let mut repair_attempted = false;
    let mut repair_succeeded = false;
    let mut citation_count = 0usize;
    let mut web_sources_reviewed_present = false;
    let mut heading_count = 0usize;
    let mut paragraph_count = 0usize;
    let mut artifact_candidates = Vec::<Value>::new();
    let mut accepted_candidate_source = None::<String>;
    let execution_mode = execution_policy
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("artifact_write");
    let parsed_status = parse_status_json(session_text);
    if rejected_reason.is_none() && matches!(execution_mode, "git_patch" | "filesystem_patch") {
        let unsafe_raw_write_paths = touched_files
            .iter()
            .filter(|path| workspace_snapshot_before.contains((*path).as_str()))
            .filter(|path| path_looks_like_source_file(path))
            .filter(|path| {
                mutation_tool_by_file
                    .get(*path)
                    .and_then(Value::as_array)
                    .is_some_and(|tools| {
                        let used_write = tools.iter().any(|value| value.as_str() == Some("write"));
                        let used_safe_patch = tools.iter().any(|value| {
                            matches!(value.as_str(), Some("edit") | Some("apply_patch"))
                        });
                        used_write && !used_safe_patch
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        if !unsafe_raw_write_paths.is_empty() {
            rejected_reason = Some(format!(
                "unsafe raw source rewrite rejected: {}",
                unsafe_raw_write_paths.join(", ")
            ));
        }
    }

    if let Some((path, text)) = accepted_output.clone() {
        let session_write_candidates =
            session_write_candidates_for_output(session, workspace_root, &path);
        let requested_has_read = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
        let executed_has_read = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
        let web_research_expected = automation_node_web_research_expected(node);
        let web_research_succeeded = tool_telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut candidate_assessments = session_write_candidates
            .iter()
            .map(|candidate| {
                assess_artifact_candidate(
                    node,
                    workspace_root,
                    "session_write",
                    candidate,
                    &read_paths,
                    &discovered_relevant_paths,
                )
            })
            .collect::<Vec<_>>();
        if !text.trim().is_empty() {
            candidate_assessments.push(assess_artifact_candidate(
                node,
                workspace_root,
                "verified_output",
                &text,
                &read_paths,
                &discovered_relevant_paths,
            ));
        }
        if let Some(previous) = preexisting_output.filter(|value| !value.trim().is_empty()) {
            candidate_assessments.push(assess_artifact_candidate(
                node,
                workspace_root,
                "preexisting_output",
                previous,
                &read_paths,
                &discovered_relevant_paths,
            ));
        }
        let best_candidate = best_artifact_candidate(&candidate_assessments);
        artifact_candidates = candidate_assessments
            .iter()
            .map(|candidate| {
                let accepted = best_candidate.as_ref().is_some_and(|best| {
                    best.source == candidate.source && best.text == candidate.text
                });
                artifact_candidate_summary(candidate, accepted)
            })
            .collect::<Vec<_>>();
        if let Some(best) = best_candidate.clone() {
            accepted_candidate_source = Some(best.source.clone());
            reviewed_paths_backed_by_read = best.reviewed_paths_backed_by_read.clone();
            citation_count = best.citation_count;
            web_sources_reviewed_present = best.web_sources_reviewed_present;
            heading_count = best.heading_count;
            paragraph_count = best.paragraph_count;
            if discovered_relevant_paths.is_empty() {
                discovered_relevant_paths = best.reviewed_paths.clone();
            }
            unreviewed_relevant_paths = best.unreviewed_relevant_paths.clone();
            let best_is_verified_output = best.source == "verified_output" && best.text == text;
            if !best_is_verified_output {
                if let Ok(resolved) = resolve_automation_output_path(workspace_root, &path) {
                    let _ = std::fs::write(&resolved, &best.text);
                }
                recovered_from_session_write = best.source == "session_write";
                accepted_output = Some((path.clone(), best.text.clone()));
            } else {
                accepted_output = Some((path.clone(), best.text.clone()));
            }
        }
        repair_attempted = session_write_candidates.len() > 1
            && (requested_has_read || web_research_expected)
            && (!reviewed_paths_backed_by_read.is_empty()
                || !read_paths.is_empty()
                || tool_telemetry
                    .get("tool_call_counts")
                    .and_then(|value| value.get("write"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 1);
        let selected_assessment = best_candidate.as_ref();
        let validator_kind = automation_output_validator_kind(node);
        let is_research_brief =
            validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief;
        let required_tools_for_node = automation_node_required_tools(node);
        let has_required_tools = !required_tools_for_node.is_empty();
        let requires_read = required_tools_for_node.iter().any(|tool| tool == "read");
        let requires_websearch = required_tools_for_node
            .iter()
            .any(|tool| tool == "websearch");
        if is_research_brief && requested_has_read {
            let missing_concrete_reads = !executed_has_read;
            let files_reviewed_backed = selected_assessment.is_some_and(|assessment| {
                !assessment.reviewed_paths.is_empty()
                    && assessment.reviewed_paths.len()
                        == assessment.reviewed_paths_backed_by_read.len()
            });
            let missing_file_coverage = !selected_assessment
                .is_some_and(|assessment| assessment.files_reviewed_present)
                || !files_reviewed_backed
                || !unreviewed_relevant_paths.is_empty();
            let missing_web_research = web_research_expected && !web_research_succeeded;
            let missing_citations =
                !selected_assessment.is_some_and(|assessment| assessment.citation_count > 0);
            let missing_web_sources_reviewed = web_research_expected
                && !selected_assessment
                    .is_some_and(|assessment| assessment.web_sources_reviewed_present);
            unmet_requirements.clear();
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_citations {
                unmet_requirements.push("citations_missing".to_string());
            }
            if !selected_assessment.is_some_and(|assessment| assessment.files_reviewed_present) {
                unmet_requirements.push("files_reviewed_missing".to_string());
            }
            if !files_reviewed_backed {
                unmet_requirements.push("files_reviewed_not_backed_by_read".to_string());
            }
            if !unreviewed_relevant_paths.is_empty() {
                unmet_requirements.push("relevant_files_not_reviewed_or_skipped".to_string());
            }
            if missing_web_sources_reviewed {
                unmet_requirements.push("web_sources_reviewed_missing".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            if missing_concrete_reads
                || missing_citations
                || missing_file_coverage
                || missing_web_sources_reviewed
                || missing_web_research
            {
                semantic_block_reason = Some(if missing_concrete_reads {
                    "research completed without concrete file reads or required source coverage"
                        .to_string()
                } else if missing_web_research {
                    "research completed without required current web research".to_string()
                } else if !unreviewed_relevant_paths.is_empty() {
                    "research completed without covering or explicitly skipping relevant discovered files".to_string()
                } else if missing_citations {
                    "research completed without citation-backed claims".to_string()
                } else if missing_web_sources_reviewed {
                    "research completed without a web sources reviewed section".to_string()
                } else {
                    "research completed without a source-backed files reviewed section".to_string()
                });
            }
        }
        if !is_research_brief && has_required_tools {
            let missing_concrete_reads = requires_read && !executed_has_read;
            let missing_web_research =
                requires_websearch && web_research_expected && !web_research_succeeded;
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            if missing_concrete_reads || missing_web_research {
                semantic_block_reason =
                    Some("artifact finalized without using required tools".to_string());
            }
        }
        if validator_kind == crate::AutomationOutputValidatorKind::GenericArtifact {
            let contract_kind = node
                .output_contract
                .as_ref()
                .map(|contract| contract.kind.trim().to_ascii_lowercase())
                .unwrap_or_default();
            let selected = selected_assessment.cloned();
            let missing_editorial_substance =
                matches!(contract_kind.as_str(), "report_markdown" | "text_summary")
                    && !selected.as_ref().is_some_and(|assessment| {
                        !assessment.placeholder_like
                            && (assessment.substantive
                                || (assessment.length >= 120 && assessment.paragraph_count >= 1))
                    });
            let missing_markdown_structure = contract_kind == "report_markdown"
                && !selected.as_ref().is_some_and(|assessment| {
                    assessment.heading_count >= 1 && assessment.paragraph_count >= 2
                });
            if missing_editorial_substance {
                unmet_requirements.push("editorial_substance_missing".to_string());
            }
            if missing_markdown_structure {
                unmet_requirements.push("markdown_structure_missing".to_string());
            }
            if semantic_block_reason.is_none()
                && (missing_editorial_substance || missing_markdown_structure)
            {
                semantic_block_reason = Some(if missing_markdown_structure {
                    "editorial artifact is missing expected markdown structure".to_string()
                } else {
                    "editorial artifact is too weak or placeholder-like".to_string()
                });
            }
        }
        if rejected_reason.is_none()
            && matches!(execution_mode, "git_patch" | "filesystem_patch")
            && preexisting_output.is_some()
            && path_looks_like_source_file(&path)
            && tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| {
                    tools.iter().any(|value| value.as_str() == Some("write"))
                        && !tools.iter().any(|value| value.as_str() == Some("edit"))
                        && !tools
                            .iter()
                            .any(|value| value.as_str() == Some("apply_patch"))
                })
        {
            rejected_reason =
                Some("code workflow used raw write without patch/edit safety".to_string());
        }
        if semantic_block_reason.is_some()
            && !recovered_from_session_write
            && selected_assessment.is_some_and(|assessment| !assessment.substantive)
        {
            // TODO(coding-hardening): Fold this recovery path into a single
            // artifact-finalization step that deterministically picks the best
            // candidate before node output is wrapped, instead of patching up the
            // final file after semantic validation fires.
            if let Some(best) = selected_assessment
                .filter(|assessment| assessment.substantive)
                .cloned()
            {
                if let Ok(resolved) = resolve_automation_output_path(workspace_root, &path) {
                    let _ = std::fs::write(&resolved, &best.text);
                }
                accepted_output = Some((path.clone(), best.text.clone()));
                recovered_from_session_write = best.source == "session_write";
                repair_succeeded = true;
                accepted_candidate_source = Some(best.source.clone());
            }
        }
        if repair_attempted && semantic_block_reason.is_none() {
            repair_succeeded = true;
        }
    }
    if accepted_output.is_some() && accepted_candidate_source.is_none() {
        accepted_candidate_source = Some("verified_output".to_string());
    }
    let (repair_attempt, repair_attempts_remaining, repair_exhausted) = infer_artifact_repair_state(
        parsed_status.as_ref(),
        repair_attempted,
        repair_succeeded,
        semantic_block_reason.as_deref(),
        tool_telemetry,
    );
    let validator_kind = automation_output_validator_kind(node);
    let has_required_tools = !automation_node_required_tools(node).is_empty();
    let validation_outcome = if ((validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief)
        || has_required_tools)
        && semantic_block_reason.is_some()
    {
        if repair_exhausted {
            "blocked"
        } else {
            "needs_repair"
        }
    } else if semantic_block_reason.is_some() {
        "blocked"
    } else {
        "passed"
    };
    let should_classify =
        validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief || has_required_tools;
    let blocking_classification = if should_classify {
        classify_research_validation_state(
            &tool_telemetry
                .get("requested_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            &tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            automation_node_web_research_expected(node),
            &unmet_requirements,
            repair_exhausted,
        )
        .map(str::to_string)
    } else {
        None
    };
    let required_next_tool_actions = if should_classify {
        research_required_next_tool_actions(
            &tool_telemetry
                .get("requested_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            &tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            automation_node_web_research_expected(node),
            &unmet_requirements,
            &unreviewed_relevant_paths,
        )
    } else {
        Vec::new()
    };

    let metadata = json!({
        "accepted_artifact_path": accepted_output.as_ref().map(|(path, _)| path.clone()),
        "accepted_candidate_source": accepted_candidate_source,
        "rejected_artifact_reason": rejected_reason,
        "semantic_block_reason": semantic_block_reason,
        "recovered_from_session_write": recovered_from_session_write,
        "undeclared_files_created": undeclared_files_created,
        "auto_cleaned": auto_cleaned,
        "execution_policy": execution_policy,
        "touched_files": touched_files,
        "mutation_tool_by_file": Value::Object(mutation_tool_by_file),
        "verification": verification_summary,
        "git_diff_summary": git_diff_summary_for_paths(workspace_root, &touched_files),
        "read_paths": read_paths,
        "discovered_relevant_paths": discovered_relevant_paths,
        "reviewed_paths_backed_by_read": reviewed_paths_backed_by_read,
        "unreviewed_relevant_paths": unreviewed_relevant_paths,
        "citation_count": citation_count,
        "web_sources_reviewed_present": web_sources_reviewed_present,
        "heading_count": heading_count,
        "paragraph_count": paragraph_count,
        "web_research_attempted": tool_telemetry.get("web_research_used").cloned().unwrap_or(json!(false)),
        "web_research_succeeded": tool_telemetry.get("web_research_succeeded").cloned().unwrap_or(json!(false)),
        "repair_attempted": repair_attempted,
        "repair_attempt": repair_attempt,
        "repair_attempts_remaining": repair_attempts_remaining,
        "repair_succeeded": repair_succeeded,
        "repair_exhausted": repair_exhausted,
        "validation_outcome": validation_outcome,
        "blocking_classification": blocking_classification,
        "required_next_tool_actions": required_next_tool_actions,
        "unmet_requirements": unmet_requirements,
        "artifact_candidates": artifact_candidates,
    });
    let rejected = metadata
        .get("rejected_artifact_reason")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            metadata
                .get("semantic_block_reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    (accepted_output, metadata, rejected)
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

fn parse_status_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return Some(value);
        }
    }
    for (idx, ch) in trimmed.char_indices().rev() {
        if ch != '{' {
            continue;
        }
        let candidate = trimmed[idx..].trim();
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            return Some(value);
        }
    }
    None
}

fn parsed_status_u32(status: Option<&Value>, key: &str) -> Option<u32> {
    status
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn infer_artifact_repair_state(
    parsed_status: Option<&Value>,
    repair_attempted: bool,
    repair_succeeded: bool,
    semantic_block_reason: Option<&str>,
    tool_telemetry: &Value,
) -> (u32, u32, bool) {
    let default_budget = tandem_core::prewrite_repair_retry_max_attempts() as u32;
    let inferred_attempt = tool_telemetry
        .get("tool_call_counts")
        .and_then(|value| value.get("write"))
        .and_then(Value::as_u64)
        .and_then(|count| count.checked_sub(1))
        .map(|count| count.min(default_budget as u64) as u32)
        .unwrap_or(0);
    let repair_attempt = parsed_status_u32(parsed_status, "repairAttempt").unwrap_or_else(|| {
        if repair_attempted {
            inferred_attempt.max(1)
        } else {
            0
        }
    });
    let repair_attempts_remaining = parsed_status_u32(parsed_status, "repairAttemptsRemaining")
        .unwrap_or_else(|| default_budget.saturating_sub(repair_attempt.min(default_budget)));
    let repair_exhausted = parsed_status
        .and_then(|value| value.get("repairExhausted"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            repair_attempted
                && !repair_succeeded
                && semantic_block_reason.is_some()
                && repair_attempt >= default_budget
        });
    (repair_attempt, repair_attempts_remaining, repair_exhausted)
}

fn summarize_automation_tool_activity(
    node: &AutomationFlowNode,
    session: &Session,
    requested_tools: &[String],
) -> Value {
    let mut executed_tools = Vec::new();
    let mut counts = serde_json::Map::new();
    let mut workspace_inspection_used = false;
    let mut web_research_used = false;
    let mut web_research_succeeded = false;
    let mut latest_web_research_failure = None::<String>;
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                error,
                result,
                ..
            } = part
            else {
                continue;
            };
            if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                let normalized = tool.trim().to_ascii_lowercase().replace('-', "_");
                if matches!(
                    normalized.as_str(),
                    "websearch" | "webfetch" | "webfetch_html"
                ) {
                    latest_web_research_failure = error
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string);
                }
                continue;
            }
            let normalized = tool.trim().to_ascii_lowercase().replace('-', "_");
            if !executed_tools.iter().any(|entry| entry == &normalized) {
                executed_tools.push(normalized.clone());
            }
            let next_count = counts
                .get(&normalized)
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .saturating_add(1);
            counts.insert(normalized.clone(), json!(next_count));
            if matches!(
                normalized.as_str(),
                "glob" | "read" | "grep" | "search" | "codesearch" | "ls" | "list"
            ) {
                workspace_inspection_used = true;
            }
            if matches!(
                normalized.as_str(),
                "websearch" | "webfetch" | "webfetch_html"
            ) {
                web_research_used = true;
                let metadata = result
                    .as_ref()
                    .and_then(|value| value.get("metadata"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let output = result
                    .as_ref()
                    .and_then(|value| value.get("output"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase();
                let result_error = metadata
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let timed_out = metadata
                    .get("error")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.eq_ignore_ascii_case("timeout"))
                    || output.contains("search timed out")
                    || output.contains("no results received");
                if result_error.is_none() && !timed_out && !output.is_empty() {
                    web_research_succeeded = true;
                    latest_web_research_failure = None;
                } else if latest_web_research_failure.is_none() {
                    latest_web_research_failure = result_error.or_else(|| {
                        if timed_out {
                            Some("web research timed out".to_string())
                        } else if output.is_empty() {
                            Some("web research returned no usable output".to_string())
                        } else {
                            Some("web research returned an unusable result".to_string())
                        }
                    });
                }
            }
        }
    }
    let verification = session_verification_summary(node, session);
    json!({
        "requested_tools": requested_tools,
        "executed_tools": executed_tools,
        "tool_call_counts": counts,
        "workspace_inspection_used": workspace_inspection_used,
        "web_research_used": web_research_used,
        "web_research_succeeded": web_research_succeeded,
        "latest_web_research_failure": latest_web_research_failure,
        "verification_expected": verification.get("verification_expected").cloned().unwrap_or(json!(false)),
        "verification_command": verification.get("verification_command").cloned().unwrap_or(Value::Null),
        "verification_plan": verification.get("verification_plan").cloned().unwrap_or(json!([])),
        "verification_results": verification.get("verification_results").cloned().unwrap_or(json!([])),
        "verification_outcome": verification.get("verification_outcome").cloned().unwrap_or(Value::Null),
        "verification_total": verification.get("verification_total").cloned().unwrap_or(json!(0)),
        "verification_completed": verification.get("verification_completed").cloned().unwrap_or(json!(0)),
        "verification_passed_count": verification.get("verification_passed_count").cloned().unwrap_or(json!(0)),
        "verification_failed_count": verification.get("verification_failed_count").cloned().unwrap_or(json!(0)),
        "verification_ran": verification.get("verification_ran").cloned().unwrap_or(json!(false)),
        "verification_failed": verification.get("verification_failed").cloned().unwrap_or(json!(false)),
        "latest_verification_command": verification.get("latest_verification_command").cloned().unwrap_or(Value::Null),
        "latest_verification_failure": verification.get("latest_verification_failure").cloned().unwrap_or(Value::Null),
    })
}

fn classify_research_validation_state(
    requested_tools: &[Value],
    executed_tools: &[Value],
    web_research_expected: bool,
    unmet_requirements: &[String],
    repair_exhausted: bool,
) -> Option<&'static str> {
    if unmet_requirements.is_empty() {
        return None;
    }
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let requested_has_websearch = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_websearch = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    if repair_exhausted {
        return Some("coverage_incomplete_after_retry");
    }
    if (!requested_has_read
        && unmet_requirements.iter().any(|value| {
            matches!(
                value.as_str(),
                "no_concrete_reads" | "concrete_read_required"
            )
        }))
        || (web_research_expected
            && !requested_has_websearch
            && unmet_requirements
                .iter()
                .any(|value| value == "missing_successful_web_research"))
    {
        return Some("tool_unavailable");
    }
    if (requested_has_read && !executed_has_read)
        || (web_research_expected && requested_has_websearch && !executed_has_websearch)
    {
        return Some("tool_available_but_not_used");
    }
    Some("tool_attempted_but_failed")
}

fn research_required_next_tool_actions(
    requested_tools: &[Value],
    executed_tools: &[Value],
    web_research_expected: bool,
    unmet_requirements: &[String],
    unreviewed_relevant_paths: &[String],
) -> Vec<String> {
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let requested_has_websearch = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_websearch = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let has_unmet = |needle: &str| unmet_requirements.iter().any(|value| value == needle);

    let mut actions = Vec::new();
    if requested_has_read
        && (!executed_has_read
            || has_unmet("no_concrete_reads")
            || has_unmet("files_reviewed_not_backed_by_read"))
    {
        if unreviewed_relevant_paths.is_empty() {
            actions.push(
                "Use `read` on concrete workspace files before finalizing the brief.".to_string(),
            );
        } else {
            actions.push(format!(
                "Use `read` on the remaining relevant workspace files: {}.",
                unreviewed_relevant_paths.join(", ")
            ));
        }
    }
    if requested_has_websearch
        && web_research_expected
        && (!executed_has_websearch
            || has_unmet("missing_successful_web_research")
            || has_unmet("web_sources_reviewed_missing"))
    {
        actions.push(
            "Use `websearch` successfully and include the resulting sources in `Web sources reviewed`."
                .to_string(),
        );
    }
    if has_unmet("citations_missing") {
        actions.push(
            "Add citation-backed proof points instead of unsupported claims before writing the final brief."
                .to_string(),
        );
    }
    if has_unmet("files_reviewed_missing") {
        actions.push(
            "Include a `Files reviewed` section that lists the exact local paths you actually read in this run."
                .to_string(),
        );
    }
    if has_unmet("relevant_files_not_reviewed_or_skipped") {
        actions.push(
            "Move every discovered relevant file into either `Files reviewed` after `read`, or `Files not reviewed` with a reason."
                .to_string(),
        );
    }
    actions
}

fn detect_automation_node_status(
    node: &AutomationFlowNode,
    session_text: &str,
    verified_output: Option<&(String, String)>,
    tool_telemetry: &Value,
    artifact_validation: Option<&Value>,
) -> (String, Option<String>, Option<bool>) {
    let research_repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let validator_kind = automation_output_validator_kind(node);
    let has_required_tools = !automation_node_required_tools(node).is_empty();
    let validation_repairable = (validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || has_required_tools)
        && !research_repair_exhausted;
    let parsed = parse_status_json(session_text);
    let approved = parsed
        .as_ref()
        .and_then(|value| value.get("approved"))
        .and_then(Value::as_bool);
    let explicit_reason = parsed
        .as_ref()
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("verify_failed"))
    {
        return (
            "verify_failed".to_string(),
            explicit_reason.or_else(|| Some("verification command failed".to_string())),
            approved,
        );
    }
    if parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("blocked"))
    {
        let has_actionable_validation = artifact_validation
            .and_then(|value| {
                value
                    .get("rejected_artifact_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .or_else(|| {
                        value
                            .get("semantic_block_reason")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                    })
            })
            .is_some();
        if !validation_repairable || !has_actionable_validation {
            return ("blocked".to_string(), explicit_reason, approved);
        }
    }
    if approved == Some(false) {
        return (
            "blocked".to_string(),
            explicit_reason
                .or_else(|| Some("upstream review did not approve the output".to_string())),
            approved,
        );
    }
    if let Some(reason) = artifact_validation.and_then(|value| {
        value
            .get("rejected_artifact_reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }) {
        return ("blocked".to_string(), Some(reason), approved);
    }
    if let Some(reason) = artifact_validation.and_then(|value| {
        value
            .get("semantic_block_reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }) {
        return (
            if validation_repairable {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(reason),
            approved,
        );
    }
    let output_text = verified_output
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| session_text.trim());
    let lowered = output_text
        .chars()
        .take(1600)
        .collect::<String>()
        .to_ascii_lowercase();
    // TODO(coding-hardening): Replace these content markers with structured node
    // status signals from the runtime/session wrapper. Prompt text should not be the
    // primary source of truth for blocked vs completed vs verify_failed decisions.
    let blocked_markers = [
        "status: blocked",
        "status blocked",
        "## status blocked",
        "blocked pending",
        "this brief is blocked",
        "brief is blocked",
        "partially blocked",
        "provisional",
        "path-level evidence",
        "based on filenames not content",
        "could not be confirmed from file contents",
        "could not safely cite exact file-derived claims",
        "not approved",
        "approval has not happened",
        "publication is blocked",
        "i’m blocked",
        "i'm blocked",
    ];
    // TODO(coding-hardening): Same here for verification failures. We should rely on
    // explicit verification result metadata and command outcomes, not phrase matching.
    let verify_failed_markers = [
        "status: verify_failed",
        "status verify_failed",
        "verification failed",
        "tests failed",
        "build failed",
        "lint failed",
        "verify failed",
    ];
    if verify_failed_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return (
            "verify_failed".to_string(),
            explicit_reason.or_else(|| Some("verification command failed".to_string())),
            approved,
        );
    }
    if blocked_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        let reason = explicit_reason.or_else(|| {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ReviewDecision
            {
                Some("review output was not approved".to_string())
            } else {
                Some("node produced a blocked handoff artifact".to_string())
            }
        });
        return ("blocked".to_string(), reason, approved);
    }
    let requested_tools = tool_telemetry
        .get("requested_tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let executed_tools = tool_telemetry
        .get("executed_tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let is_brief_contract = validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief;
    let requires_read = automation_node_required_tools(node)
        .iter()
        .any(|value| value == "read");
    let verification_expected = tool_telemetry
        .get("verification_expected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_ran = tool_telemetry
        .get("verification_ran")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_failed = tool_telemetry
        .get("verification_failed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_outcome = tool_telemetry
        .get("verification_outcome")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let verification_completed = tool_telemetry
        .get("verification_completed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let verification_total = tool_telemetry
        .get("verification_total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let verification_failure_reason = tool_telemetry
        .get("latest_verification_failure")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if verification_expected && verification_failed {
        return (
            "verify_failed".to_string(),
            explicit_reason.or(verification_failure_reason),
            approved,
        );
    }
    if automation_node_is_code_workflow(node)
        && verification_expected
        && verification_outcome.as_deref() == Some("partial")
    {
        return (
            "blocked".to_string(),
            Some(format!(
                "coding task completed with only {} of {} declared verification commands run",
                verification_completed, verification_total
            )),
            approved,
        );
    }
    if automation_node_is_code_workflow(node) && verification_expected && !verification_ran {
        return (
            "blocked".to_string(),
            Some(
                "coding task completed without running the declared verification command"
                    .to_string(),
            ),
            approved,
        );
    }
    let mentions_missing_file_evidence = lowered.contains("file contents were not")
        || lowered.contains("could not safely cite exact file-derived claims")
        || lowered.contains("could not be confirmed from file contents")
        || lowered.contains("path-level evidence")
        || lowered.contains("based on filenames not content")
        || lowered.contains("partially blocked")
        || lowered.contains("provisional")
        || lowered.contains("this brief is blocked")
        || lowered.contains("brief is blocked");
    if (is_brief_contract && requested_has_read && !executed_has_read)
        || (requires_read && requested_has_read && !executed_has_read)
    {
        return (
            if validation_repairable {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(if mentions_missing_file_evidence {
                if is_brief_contract {
                    "research brief did not read concrete workspace files, so source-backed validation is incomplete".to_string()
                } else {
                    "node did not use required read tool calls before finalizing the artifact"
                        .to_string()
                }
            } else {
                if is_brief_contract {
                    "research brief cited workspace sources without using read, so source-backed validation is incomplete".to_string()
                } else {
                    "node finalized its artifact without required concrete file reads".to_string()
                }
            }),
            approved,
        );
    }
    if automation_node_is_code_workflow(node) {
        return ("done".to_string(), explicit_reason, approved);
    }
    ("completed".to_string(), explicit_reason, approved)
}

fn automation_node_workflow_class(node: &AutomationFlowNode) -> String {
    if automation_node_is_code_workflow(node) {
        "code".to_string()
    } else if automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
    {
        "research".to_string()
    } else {
        "artifact".to_string()
    }
}

fn detect_automation_node_failure_kind(
    node: &AutomationFlowNode,
    status: &str,
    approved: Option<bool>,
    blocked_reason: Option<&str>,
    artifact_validation: Option<&Value>,
) -> Option<String> {
    let normalized_status = status.trim().to_ascii_lowercase();
    let reason = blocked_reason
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let unmet_requirements = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_unmet = |needle: &str| {
        unmet_requirements
            .iter()
            .any(|value| value.as_str() == Some(needle))
    };
    let has_required_tools = !automation_node_required_tools(node).is_empty();
    let research_requirements_blocked = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
        && (has_unmet("no_concrete_reads")
            || has_unmet("concrete_read_required")
            || has_unmet("missing_successful_web_research")
            || has_unmet("citations_missing")
            || has_unmet("web_sources_reviewed_missing")
            || has_unmet("files_reviewed_missing")
            || has_unmet("files_reviewed_not_backed_by_read")
            || has_unmet("relevant_files_not_reviewed_or_skipped")
            || has_unmet("coverage_mode"));
    let required_tools_blocked = has_required_tools
        && (has_unmet("no_concrete_reads")
            || has_unmet("concrete_read_required")
            || has_unmet("missing_successful_web_research"));
    let editorial_requirements_blocked = has_unmet("editorial_substance_missing")
        || has_unmet("markdown_structure_missing")
        || has_unmet("editorial_clearance_required");
    let verification_failed = artifact_validation
        .and_then(|value| value.get("verification"))
        .and_then(|value| value.get("verification_failed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if verification_failed || normalized_status == "verify_failed" {
        return Some("verification_failed".to_string());
    }
    if let Some(rejected_reason) = artifact_validation
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if rejected_reason.contains("placeholder") {
            return Some("placeholder_overwrite_rejected".to_string());
        }
        if rejected_reason.contains("unsafe raw source rewrite")
            || rejected_reason.contains("raw write without patch/edit")
        {
            return Some("unsafe_raw_write_rejected".to_string());
        }
        return Some("artifact_rejected".to_string());
    }
    if artifact_validation
        .and_then(|value| value.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .is_some()
        || (automation_output_validator_kind(node)
            == crate::AutomationOutputValidatorKind::ResearchBrief
            && matches!(normalized_status.as_str(), "blocked" | "needs_repair")
            && research_requirements_blocked)
        || (has_required_tools
            && matches!(normalized_status.as_str(), "blocked" | "needs_repair")
            && required_tools_blocked)
        || (automation_output_validator_kind(node)
            == crate::AutomationOutputValidatorKind::GenericArtifact
            && normalized_status == "blocked"
            && editorial_requirements_blocked)
    {
        let repair_exhausted = artifact_validation
            .and_then(|value| value.get("repair_exhausted"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if repair_exhausted && research_requirements_blocked {
            return Some("research_retry_exhausted".to_string());
        }
        if has_unmet("no_concrete_reads") || has_unmet("concrete_read_required") {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ResearchBrief
            {
                return Some("research_missing_reads".to_string());
            }
            return Some("required_tool_unused_read".to_string());
        }
        if has_unmet("missing_successful_web_research") {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ResearchBrief
            {
                return Some("research_missing_web_research".to_string());
            }
            return Some("required_tool_unused_websearch".to_string());
        }
        if has_unmet("citations_missing") || has_unmet("web_sources_reviewed_missing") {
            return Some("research_citations_missing".to_string());
        }
        if has_unmet("files_reviewed_missing")
            || has_unmet("files_reviewed_not_backed_by_read")
            || has_unmet("relevant_files_not_reviewed_or_skipped")
            || has_unmet("coverage_mode")
        {
            return Some("research_coverage_failed".to_string());
        }
        if editorial_requirements_blocked {
            return Some("editorial_quality_failed".to_string());
        }
        return Some("semantic_blocked".to_string());
    }
    if normalized_status == "blocked" && approved == Some(false) {
        return Some("review_not_approved".to_string());
    }
    if normalized_status == "blocked" && reason.contains("upstream review did not approve") {
        return Some("upstream_not_approved".to_string());
    }
    if normalized_status == "failed" {
        return Some("run_failed".to_string());
    }
    if automation_node_is_code_workflow(node) && normalized_status == "done" {
        return Some("verification_passed".to_string());
    }
    None
}

fn build_automation_validator_summary(
    validator_kind: crate::AutomationOutputValidatorKind,
    status: &str,
    blocked_reason: Option<&str>,
    artifact_validation: Option<&Value>,
) -> crate::AutomationValidatorSummary {
    let normalized_status = status.trim().to_ascii_lowercase();
    let verification_outcome = artifact_validation
        .and_then(|value| value.get("verification"))
        .and_then(|value| {
            value
                .get("verification_outcome")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    if value
                        .get("verification_failed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        Some("failed".to_string())
                    } else if value
                        .get("verification_ran")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        Some("passed".to_string())
                    } else {
                        None
                    }
                })
        });
    let unmet_requirements = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let accepted_candidate_source = artifact_validation
        .and_then(|value| value.get("accepted_candidate_source"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let repair_attempted = artifact_validation
        .and_then(|value| value.get("repair_attempted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| tandem_core::prewrite_repair_retry_max_attempts() as u32);
    let repair_succeeded = artifact_validation
        .and_then(|value| value.get("repair_succeeded"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reason = blocked_reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("rejected_artifact_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    let outcome = match normalized_status.as_str() {
        "completed" | "done" => "passed",
        "verify_failed" => "verify_failed",
        "blocked" => "blocked",
        "failed" => "failed",
        other => other,
    }
    .to_string();
    crate::AutomationValidatorSummary {
        kind: validator_kind,
        outcome,
        reason,
        unmet_requirements,
        accepted_candidate_source,
        verification_outcome,
        repair_attempted,
        repair_attempt,
        repair_attempts_remaining,
        repair_succeeded,
        repair_exhausted,
    }
}

pub(crate) fn enrich_automation_node_output_for_contract(
    node: &AutomationFlowNode,
    output: &Value,
) -> Value {
    let Some(mut object) = output.as_object().cloned() else {
        return output.clone();
    };
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed")
        .to_string();
    let blocked_reason = object
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let approved = object
        .get("approved")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let artifact_validation = object.get("artifact_validation").cloned();
    let validator_kind = automation_output_validator_kind(node);

    object.insert(
        "contract_kind".to_string(),
        json!(node
            .output_contract
            .as_ref()
            .map(|row| row.kind.clone())
            .unwrap_or_else(|| "structured_json".to_string())),
    );
    object.insert("validator_kind".to_string(), json!(validator_kind));
    object.insert(
        "workflow_class".to_string(),
        json!(automation_node_workflow_class(node)),
    );
    object.insert(
        "phase".to_string(),
        json!(detect_automation_node_phase(
            node,
            &status,
            artifact_validation.as_ref()
        )),
    );
    object.insert(
        "failure_kind".to_string(),
        detect_automation_node_failure_kind(
            node,
            &status,
            Some(approved),
            blocked_reason.as_deref(),
            artifact_validation.as_ref(),
        )
        .map(Value::String)
        .unwrap_or(Value::Null),
    );
    object.insert(
        "validator_summary".to_string(),
        json!(build_automation_validator_summary(
            validator_kind,
            &status,
            blocked_reason.as_deref(),
            artifact_validation.as_ref(),
        )),
    );
    Value::Object(object)
}

fn detect_automation_node_phase(
    node: &AutomationFlowNode,
    status: &str,
    artifact_validation: Option<&Value>,
) -> String {
    let workflow_class = automation_node_workflow_class(node);
    let normalized_status = status.trim().to_ascii_lowercase();
    match workflow_class.as_str() {
        "research" => {
            let unmet_requirements = artifact_validation
                .and_then(|value| value.get("unmet_requirements"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let has_unmet = |needle: &str| {
                unmet_requirements
                    .iter()
                    .any(|value| value.as_str() == Some(needle))
            };
            let research_validation_blocked = artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .is_some()
                || (automation_output_validator_kind(node)
                    == crate::AutomationOutputValidatorKind::ResearchBrief
                    && normalized_status == "blocked"
                    && (has_unmet("no_concrete_reads")
                        || has_unmet("concrete_read_required")
                        || has_unmet("missing_successful_web_research")
                        || has_unmet("citations_missing")
                        || has_unmet("web_sources_reviewed_missing")
                        || has_unmet("files_reviewed_missing")
                        || has_unmet("files_reviewed_not_backed_by_read")
                        || has_unmet("relevant_files_not_reviewed_or_skipped")
                        || has_unmet("coverage_mode")));
            if research_validation_blocked {
                "research_validation".to_string()
            } else if normalized_status == "completed" {
                "completed".to_string()
            } else {
                "research".to_string()
            }
        }
        "code" => {
            let verification_expected = artifact_validation
                .and_then(|value| value.get("verification"))
                .and_then(|value| value.get("verification_expected"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if verification_expected {
                if normalized_status == "done" {
                    "completed".to_string()
                } else {
                    "verification".to_string()
                }
            } else if normalized_status == "done" {
                "completed".to_string()
            } else {
                "implementation".to_string()
            }
        }
        _ => {
            let unmet_requirements = artifact_validation
                .and_then(|value| value.get("unmet_requirements"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let has_unmet = |needle: &str| {
                unmet_requirements
                    .iter()
                    .any(|value| value.as_str() == Some(needle))
            };
            let editorial_validation_blocked = (has_unmet("editorial_substance_missing")
                || has_unmet("markdown_structure_missing")
                || has_unmet("editorial_clearance_required"))
                && (artifact_validation
                    .and_then(|value| value.get("semantic_block_reason"))
                    .and_then(Value::as_str)
                    .is_some()
                    || normalized_status == "blocked");
            if editorial_validation_blocked {
                "editorial_validation".to_string()
            } else if normalized_status == "completed" {
                "completed".to_string()
            } else {
                "artifact_write".to_string()
            }
        }
    }
}

fn wrap_automation_node_output(
    node: &AutomationFlowNode,
    session: &Session,
    requested_tools: &[String],
    session_id: &str,
    session_text: &str,
    verified_output: Option<(String, String)>,
    artifact_validation: Option<Value>,
) -> Value {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.clone())
        .unwrap_or_else(|| "structured_json".to_string());
    let summary = if let Some((path, _)) = verified_output.as_ref() {
        format!(
            "Verified workspace output `{}` for node `{}`.",
            path, node.node_id
        )
    } else if let Some(reason) = artifact_validation
        .as_ref()
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        format!(
            "Artifact validation rejected node `{}` output: {}.",
            node.node_id, reason
        )
    } else if session_text.trim().is_empty() {
        format!("Node `{}` completed successfully.", node.node_id)
    } else {
        truncate_text(session_text.trim(), 240)
    };
    let primary_text = verified_output
        .as_ref()
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| session_text.trim());
    let tool_telemetry = summarize_automation_tool_activity(node, session, requested_tools);
    let (status, blocked_reason, approved) = detect_automation_node_status(
        node,
        session_text,
        verified_output.as_ref(),
        &tool_telemetry,
        artifact_validation.as_ref(),
    );
    let workflow_class = automation_node_workflow_class(node);
    let validator_kind = automation_output_validator_kind(node);
    let phase = detect_automation_node_phase(node, &status, artifact_validation.as_ref());
    let failure_kind = detect_automation_node_failure_kind(
        node,
        &status,
        approved,
        blocked_reason.as_deref(),
        artifact_validation.as_ref(),
    );
    let validator_summary = build_automation_validator_summary(
        validator_kind,
        &status,
        blocked_reason.as_deref(),
        artifact_validation.as_ref(),
    );
    let content = match contract_kind.as_str() {
        "report_markdown" | "text_summary" => {
            json!({
                "text": primary_text,
                "path": verified_output.as_ref().map(|(path, _)| path.clone()),
                "raw_assistant_text": session_text.trim(),
                "session_id": session_id
            })
        }
        "urls" => json!({
            "items": [],
            "raw_text": primary_text,
            "path": verified_output.as_ref().map(|(path, _)| path.clone()),
            "raw_assistant_text": session_text.trim(),
            "session_id": session_id
        }),
        "citations" => {
            json!({
                "items": [],
                "raw_text": primary_text,
                "path": verified_output.as_ref().map(|(path, _)| path.clone()),
                "raw_assistant_text": session_text.trim(),
                "session_id": session_id
            })
        }
        _ => json!({
            "text": primary_text,
            "path": verified_output.as_ref().map(|(path, _)| path.clone()),
            "raw_assistant_text": session_text.trim(),
            "session_id": session_id
        }),
    };
    json!(AutomationNodeOutput {
        contract_kind,
        validator_kind: Some(validator_kind),
        validator_summary: Some(validator_summary),
        summary,
        content,
        created_at_ms: now_ms(),
        node_id: node.node_id.clone(),
        status: Some(status),
        blocked_reason,
        approved,
        workflow_class: Some(workflow_class),
        phase: Some(phase),
        failure_kind,
        tool_telemetry: Some(tool_telemetry),
        artifact_validation,
    })
}

async fn record_automation_external_actions_for_session(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    attempt: u32,
    session_id: &str,
    session: &Session,
) -> anyhow::Result<Vec<ExternalActionRecord>> {
    let actions = collect_automation_external_action_receipts(
        &state.capability_resolver.list_bindings().await?,
        run_id,
        automation,
        node,
        attempt,
        session_id,
        session,
    );
    let mut recorded = Vec::with_capacity(actions.len());
    for action in actions {
        recorded.push(state.record_external_action(action).await?);
    }
    Ok(recorded)
}

fn collect_automation_external_action_receipts(
    bindings: &capability_resolver::CapabilityBindingsFile,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    attempt: u32,
    session_id: &str,
    session: &Session,
) -> Vec<ExternalActionRecord> {
    if !automation_node_is_outbound_action(node) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (call_index, part) in session
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .enumerate()
    {
        let MessagePart::ToolInvocation {
            tool,
            args,
            result,
            error,
        } = part
        else {
            continue;
        };
        if error.as_ref().is_some_and(|value| !value.trim().is_empty()) || result.is_none() {
            continue;
        }
        let Some(binding) = bindings
            .bindings
            .iter()
            .find(|binding| automation_binding_matches_tool_name(binding, tool))
        else {
            continue;
        };
        let idempotency_key = crate::sha256_hex(&[
            "automation_v2",
            &automation.automation_id,
            run_id,
            &node.node_id,
            &attempt.to_string(),
            tool,
            &args.to_string(),
            &call_index.to_string(),
        ]);
        if !seen.insert(idempotency_key.clone()) {
            continue;
        }
        let source_id = format!("{run_id}:{}:{attempt}:{call_index}", node.node_id);
        let created_at_ms = now_ms();
        out.push(ExternalActionRecord {
            action_id: format!("automation-external-{}", &idempotency_key[..16]),
            operation: binding.capability_id.clone(),
            status: "posted".to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some(source_id),
            routine_run_id: None,
            context_run_id: Some(format!("automation-v2-{run_id}")),
            capability_id: Some(binding.capability_id.clone()),
            provider: Some(binding.provider.clone()),
            target: automation_external_action_target(args, result.as_ref()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some(idempotency_key),
            receipt: Some(json!({
                "tool": tool,
                "args": args,
                "result": result,
            })),
            error: None,
            metadata: Some(json!({
                "automationID": automation.automation_id,
                "automationRunID": run_id,
                "nodeID": node.node_id,
                "attempt": attempt,
                "nodeObjective": node.objective,
                "sessionID": session_id,
                "tool": tool,
                "provider": binding.provider,
            })),
            created_at_ms,
            updated_at_ms: created_at_ms,
        });
    }
    out
}

fn automation_node_is_outbound_action(node: &AutomationFlowNode) -> bool {
    if node
        .metadata
        .as_ref()
        .and_then(|value| value.pointer("/builder/role"))
        .and_then(Value::as_str)
        .is_some_and(|role| role.eq_ignore_ascii_case("publisher"))
    {
        return true;
    }
    let objective = node.objective.to_ascii_lowercase();
    [
        "publish", "post ", "send ", "notify", "deliver", "submit", "share",
    ]
    .iter()
    .any(|needle| objective.contains(needle))
}

fn automation_publish_editorial_block_reason(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
) -> Option<String> {
    if !automation_node_is_outbound_action(node) {
        return None;
    }
    let mut upstream_ids = node.depends_on.clone();
    for input in &node.input_refs {
        if !upstream_ids
            .iter()
            .any(|value| value == &input.from_step_id)
        {
            upstream_ids.push(input.from_step_id.clone());
        }
    }
    let blocked_upstreams = upstream_ids
        .into_iter()
        .filter(|node_id| {
            let Some(output) = run.checkpoint.node_outputs.get(node_id) else {
                return false;
            };
            output
                .get("failure_kind")
                .and_then(Value::as_str)
                .is_some_and(|value| value == "editorial_quality_failed")
                || output
                    .get("phase")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == "editorial_validation")
                || output
                    .get("validator_summary")
                    .and_then(|value| value.get("unmet_requirements"))
                    .and_then(Value::as_array)
                    .is_some_and(|requirements| {
                        requirements.iter().any(|value| {
                            matches!(
                                value.as_str(),
                                Some("editorial_substance_missing")
                                    | Some("markdown_structure_missing")
                                    | Some("editorial_clearance_required")
                            )
                        })
                    })
        })
        .collect::<Vec<_>>();
    if blocked_upstreams.is_empty() {
        None
    } else {
        Some(format!(
            "publish step blocked until upstream editorial issues are resolved: {}",
            blocked_upstreams.join(", ")
        ))
    }
}

fn automation_binding_matches_tool_name(
    binding: &capability_resolver::CapabilityBinding,
    tool_name: &str,
) -> bool {
    binding.tool_name.eq_ignore_ascii_case(tool_name)
        || binding
            .tool_name_aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(tool_name))
}

fn automation_external_action_target(args: &Value, result: Option<&Value>) -> Option<String> {
    for candidate in [
        args.pointer("/owner_repo").and_then(Value::as_str),
        args.pointer("/repo").and_then(Value::as_str),
        args.pointer("/repository").and_then(Value::as_str),
        args.pointer("/channel").and_then(Value::as_str),
        args.pointer("/channel_id").and_then(Value::as_str),
        args.pointer("/thread_ts").and_then(Value::as_str),
        result
            .and_then(|value| value.pointer("/metadata/channel"))
            .and_then(Value::as_str),
        result
            .and_then(|value| value.pointer("/metadata/repo"))
            .and_then(Value::as_str),
    ] {
        let trimmed = candidate.map(str::trim).unwrap_or_default();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub(crate) fn automation_node_max_attempts(node: &AutomationFlowNode) -> u32 {
    let explicit = node
        .retry_policy
        .as_ref()
        .and_then(|value| value.get("max_attempts"))
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 10) as u32);
    if let Some(value) = explicit {
        return value;
    }
    if automation_output_validator_kind(node) == crate::AutomationOutputValidatorKind::ResearchBrief
        || !automation_node_required_tools(node).is_empty()
    {
        5
    } else {
        3
    }
}

pub(crate) fn automation_output_is_blocked(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("blocked"))
}

pub(crate) fn automation_output_is_verify_failed(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("verify_failed"))
}

pub(crate) fn automation_output_needs_repair(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("needs_repair"))
}

pub(crate) fn automation_output_repair_exhausted(output: &Value) -> bool {
    output
        .get("artifact_validation")
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn automation_output_failure_reason(output: &Value) -> Option<String> {
    output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn automation_output_blocked_reason(output: &Value) -> Option<String> {
    output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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

fn automation_declared_output_paths(automation: &AutomationV2Spec) -> Vec<String> {
    let mut paths = Vec::new();
    for target in &automation.output_targets {
        let trimmed = target.trim();
        if !trimmed.is_empty() && !paths.iter().any(|existing| existing == trimmed) {
            paths.push(trimmed.to_string());
        }
    }
    for node in &automation.flow.nodes {
        if let Some(path) = automation_node_required_output_path(node) {
            let trimmed = path.trim();
            if !trimmed.is_empty() && !paths.iter().any(|existing| existing == trimmed) {
                paths.push(trimmed.to_string());
            }
        }
    }
    paths
}

pub(crate) async fn clear_automation_declared_outputs(
    state: &AppState,
    automation: &AutomationV2Spec,
) -> anyhow::Result<()> {
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    // Preserve existing declared outputs across fresh runs so a failed retry does not
    // wipe the user's last substantive artifacts. Descendant retry/requeue paths still
    // clear subtree outputs explicitly when we know which node is being reset.
    let _ = automation_declared_output_paths(automation);
    remove_suspicious_automation_marker_files(&workspace_root);
    Ok(())
}

pub async fn clear_automation_subtree_outputs(
    state: &AppState,
    automation: &AutomationV2Spec,
    node_ids: &std::collections::HashSet<String>,
) -> anyhow::Result<Vec<String>> {
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    let mut cleared = Vec::new();
    for node in &automation.flow.nodes {
        if !node_ids.contains(&node.node_id) {
            continue;
        }
        let Some(output_path) = automation_node_required_output_path(node) else {
            continue;
        };
        let resolved = resolve_automation_output_path(&workspace_root, &output_path)?;
        if resolved.exists() && resolved.is_file() {
            std::fs::remove_file(&resolved).map_err(|error| {
                anyhow::anyhow!(
                    "failed to clear subtree output `{}` for automation `{}`: {}",
                    output_path,
                    automation.automation_id,
                    error
                )
            })?;
            cleared.push(output_path);
        }
    }
    let had_markers = !list_suspicious_automation_marker_files(&workspace_root).is_empty();
    if had_markers {
        remove_suspicious_automation_marker_files(&workspace_root);
    }
    cleared.sort();
    cleared.dedup();
    Ok(cleared)
}

pub(crate) async fn execute_automation_v2_node(
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
    let attempt = run
        .checkpoint
        .node_attempts
        .get(&node.node_id)
        .copied()
        .unwrap_or(1);
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
    let template = if let Some(template_id) = agent.template_id.as_deref().map(str::trim) {
        if template_id.is_empty() {
            None
        } else {
            state
                .agent_teams
                .get_template_for_workspace(&workspace_root, template_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("agent template `{}` not found", template_id))
                .map(Some)?
        }
    } else {
        None
    };
    let mut session = Session::new(
        Some(format!(
            "Automation {} / {}",
            automation.automation_id, node.node_id
        )),
        Some(workspace_root.clone()),
    );
    let session_id = session.id.clone();
    let project_id = automation_workspace_project_id(&workspace_root);
    session.project_id = Some(project_id.clone());
    session.workspace_root = Some(workspace_root.clone());
    state.storage.save_session(session).await?;

    state.add_automation_v2_session(run_id, &session_id).await;

    let mut allowlist = merge_automation_agent_allowlist(agent, template.as_ref());
    if let Some(mcp_tools) = agent.mcp_policy.allowed_tools.as_ref() {
        allowlist.extend(mcp_tools.clone());
    }
    let requested_tools =
        normalize_automation_requested_tools(node, &workspace_root, allowlist.clone());
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, requested_tools.clone())
        .await;
    state
        .engine_loop
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    let model = resolve_automation_agent_model(agent, template.as_ref());
    let required_output_path = automation_node_required_output_path(node);
    let preexisting_output = required_output_path
        .as_deref()
        .and_then(|output_path| resolve_automation_output_path(&workspace_root, output_path).ok())
        .and_then(|resolved| std::fs::read_to_string(resolved).ok());
    let workspace_snapshot_before = automation_workspace_root_file_snapshot(&workspace_root);
    let standup_report_path = if is_agent_standup_automation(automation)
        && node.node_id == "standup_synthesis"
    {
        resolve_standup_report_path_for_run(automation, run.started_at_ms.unwrap_or_else(now_ms))
    } else {
        None
    };
    let max_attempts = automation_node_max_attempts(node);
    let mut prompt = render_automation_v2_prompt(
        automation,
        &workspace_root,
        run_id,
        node,
        attempt,
        agent,
        &upstream_inputs,
        template
            .as_ref()
            .and_then(|value| value.system_prompt.as_deref()),
        standup_report_path.as_deref(),
        if is_agent_standup_automation(automation) {
            Some(project_id.as_str())
        } else {
            None
        },
    );
    if let Some(repair_brief) = render_automation_repair_brief(
        node,
        run.checkpoint.node_outputs.get(&node.node_id),
        attempt,
        max_attempts,
    ) {
        prompt.push_str("\n\n");
        prompt.push_str(&repair_brief);
    }
    let req = SendMessageRequest {
        parts: vec![MessagePartInput::Text { text: prompt }],
        model,
        agent: None,
        tool_mode: Some(ToolMode::Required),
        tool_allowlist: Some(requested_tools.clone()),
        context_mode: None,
        write_required: required_output_path.as_ref().map(|_| true),
        prewrite_requirements: automation_node_prewrite_requirements(node, &requested_tools),
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
    let verified_output = if let Some(output_path) = required_output_path.as_deref() {
        let resolved = resolve_automation_output_path(&workspace_root, output_path)?;
        if !resolved.exists() {
            anyhow::bail!(
                "required output `{}` was not created for node `{}`",
                output_path,
                node.node_id
            );
        }
        if !resolved.is_file() {
            anyhow::bail!(
                "required output `{}` for node `{}` is not a file",
                output_path,
                node.node_id
            );
        }
        let file_text = std::fs::read_to_string(&resolved).map_err(|error| {
            anyhow::anyhow!(
                "required output `{}` for node `{}` could not be read: {}",
                output_path,
                node.node_id,
                error
            )
        })?;
        Some((output_path.to_string(), file_text))
    } else {
        None
    };
    let tool_telemetry = summarize_automation_tool_activity(node, &session, &requested_tools);
    let (verified_output, mut artifact_validation, artifact_rejected_reason) =
        validate_automation_artifact_output(
            node,
            &session,
            &workspace_root,
            &session_text,
            &tool_telemetry,
            preexisting_output.as_deref(),
            verified_output,
            &workspace_snapshot_before,
        );
    let _ = artifact_rejected_reason;
    let editorial_publish_block_reason = state
        .get_automation_v2_run(run_id)
        .await
        .and_then(|run| automation_publish_editorial_block_reason(&run, node));
    if let Some(reason) = editorial_publish_block_reason.as_ref() {
        if let Some(object) = artifact_validation.as_object_mut() {
            let unmet = object
                .entry("unmet_requirements".to_string())
                .or_insert_with(|| json!([]));
            if let Some(rows) = unmet.as_array_mut() {
                if !rows
                    .iter()
                    .any(|value| value.as_str() == Some("editorial_clearance_required"))
                {
                    rows.push(json!("editorial_clearance_required"));
                }
            }
            object
                .entry("semantic_block_reason".to_string())
                .or_insert_with(|| Value::String(reason.clone()));
        }
    }
    let external_actions = if editorial_publish_block_reason.is_some() {
        Vec::new()
    } else {
        record_automation_external_actions_for_session(
            state,
            run_id,
            automation,
            node,
            attempt,
            &session_id,
            &session,
        )
        .await?
    };
    let mut output = wrap_automation_node_output(
        node,
        &session,
        &requested_tools,
        &session_id,
        &session_text,
        verified_output,
        Some(artifact_validation),
    );
    if !external_actions.is_empty() {
        if let Some(object) = output.as_object_mut() {
            object.insert(
                "external_actions".to_string(),
                serde_json::to_value(&external_actions)?,
            );
        }
    }
    Ok(output)
}

pub async fn run_automation_v2_executor(state: AppState) {
    crate::automation_v2::executor::run_automation_v2_executor(state).await
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_automation_node(
        node_id: &str,
        depends_on: Vec<&str>,
        phase_id: &str,
        priority: i64,
    ) -> AutomationFlowNode {
        AutomationFlowNode {
            node_id: node_id.to_string(),
            agent_id: "agent-a".to_string(),
            objective: format!("Run {node_id}"),
            depends_on: depends_on.into_iter().map(str::to_string).collect(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "phase_id": phase_id,
                    "priority": priority
                }
            })),
        }
    }

    fn test_phase_automation(phases: Value, nodes: Vec<AutomationFlowNode>) -> AutomationV2Spec {
        AutomationV2Spec {
            automation_id: "auto-phase-test".to_string(),
            name: "Phase Test".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
            },
            agents: vec![AutomationAgentProfile {
                agent_id: "agent-a".to_string(),
                template_id: Some("template-a".to_string()),
                display_name: "Agent A".to_string(),
                avatar_url: None,
                model_policy: None,
                skills: Vec::new(),
                tool_policy: AutomationAgentToolPolicy {
                    allowlist: Vec::new(),
                    denylist: Vec::new(),
                },
                mcp_policy: AutomationAgentMcpPolicy {
                    allowed_servers: Vec::new(),
                    allowed_tools: None,
                },
                approval_policy: None,
            }],
            flow: AutomationFlowSpec { nodes },
            execution: AutomationExecutionPolicy {
                max_parallel_agents: Some(2),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: "test".to_string(),
            workspace_root: Some(".".to_string()),
            metadata: Some(json!({
                "mission": {
                    "phases": phases
                }
            })),
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        }
    }

    fn test_phase_run(
        pending_nodes: Vec<&str>,
        completed_nodes: Vec<&str>,
    ) -> AutomationV2RunRecord {
        AutomationV2RunRecord {
            run_id: "run-phase-test".to_string(),
            automation_id: "auto-phase-test".to_string(),
            trigger_type: "manual".to_string(),
            status: AutomationRunStatus::Queued,
            created_at_ms: 1,
            updated_at_ms: 1,
            started_at_ms: None,
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: completed_nodes.into_iter().map(str::to_string).collect(),
                pending_nodes: pending_nodes.into_iter().map(str::to_string).collect(),
                node_outputs: std::collections::HashMap::new(),
                node_attempts: std::collections::HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            automation_snapshot: None,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        }
    }

    fn test_state_with_path(path: PathBuf) -> AppState {
        let mut state = AppState::new_starting("test-attempt".to_string(), true);
        state.shared_resources_path = path;
        state.routines_path = tmp_routines_file("shared-state");
        state.routine_history_path = tmp_routines_file("routine-history");
        state.routine_runs_path = tmp_routines_file("routine-runs");
        state.external_actions_path = tmp_routines_file("external-actions");
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
    fn derive_status_index_update_for_failed_write_includes_recovery_snapshot() {
        let event = EngineEvent::new(
            "message.part.updated",
            serde_json::json!({
                "sessionID": "s-3",
                "runID": "r-3",
                "part": {
                    "id": "call_stream_1",
                    "type": "tool",
                    "state": "failed",
                    "tool": "write",
                    "args": {
                        "path": "game.html",
                        "content": "<html>draft</html>"
                    },
                    "error": "WRITE_ARGS_EMPTY_FROM_PROVIDER"
                }
            }),
        );
        let update = derive_status_index_update(&event).expect("update");
        assert_eq!(update.key, "run/s-3/status");
        assert_eq!(
            update.value.get("phase").and_then(|v| v.as_str()),
            Some("run")
        );
        assert_eq!(
            update.value.get("toolActive").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            update.value.get("tool").and_then(|v| v.as_str()),
            Some("write")
        );
        assert_eq!(
            update.value.get("toolState").and_then(|v| v.as_str()),
            Some("failed")
        );
        assert_eq!(
            update.value.get("toolError").and_then(|v| v.as_str()),
            Some("WRITE_ARGS_EMPTY_FROM_PROVIDER")
        );
        assert_eq!(
            update.value.get("toolCallID").and_then(|v| v.as_str()),
            Some("call_stream_1")
        );
        let preview = update
            .value
            .get("toolArgsPreview")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(preview.contains("game.html"));
        assert!(preview.contains("<html>draft</html>"));
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
        let _ = tokio::fs::remove_file(config::paths::sibling_backup_path(&routines_path)).await;
    }

    #[tokio::test]
    async fn load_routines_recovers_from_backup_when_primary_corrupt() {
        let routines_path = tmp_routines_file("backup-recovery");
        let backup_path = config::paths::sibling_backup_path(&routines_path);
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
    async fn record_external_action_appends_routine_receipt_artifact() {
        let state = test_state_with_path(tmp_resource_file("external-action-artifact"));
        let run = RoutineRunRecord {
            run_id: "run-1".to_string(),
            routine_id: "routine-1".to_string(),
            trigger_type: "manual".to_string(),
            run_count: 1,
            status: RoutineRunStatus::Completed,
            created_at_ms: 1,
            updated_at_ms: 1,
            fired_at_ms: Some(1),
            started_at_ms: Some(1),
            finished_at_ms: Some(1),
            requires_approval: false,
            approval_reason: None,
            denial_reason: None,
            paused_reason: None,
            detail: None,
            entrypoint: "workflow.publish".to_string(),
            args: Value::Null,
            allowed_tools: Vec::new(),
            output_targets: Vec::new(),
            artifacts: Vec::new(),
            active_session_ids: Vec::new(),
            latest_session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        };
        state
            .routine_runs
            .write()
            .await
            .insert(run.run_id.clone(), run);

        state
            .record_external_action(ExternalActionRecord {
                action_id: "action-1".to_string(),
                operation: "create_issue".to_string(),
                status: "posted".to_string(),
                source_kind: Some("bug_monitor".to_string()),
                source_id: Some("draft-1".to_string()),
                routine_run_id: Some("run-1".to_string()),
                context_run_id: None,
                capability_id: Some("github.create_issue".to_string()),
                provider: Some("bug-monitor".to_string()),
                target: Some("acme/platform".to_string()),
                approval_state: Some("executed".to_string()),
                idempotency_key: Some("idem-1".to_string()),
                receipt: Some(json!({"issue_number": 101})),
                error: None,
                metadata: None,
                created_at_ms: 10,
                updated_at_ms: 10,
            })
            .await
            .expect("record external action");

        let updated = state.get_routine_run("run-1").await.expect("routine run");
        assert_eq!(updated.artifacts.len(), 1);
        assert_eq!(updated.artifacts[0].kind, "external_action_receipt");
        assert_eq!(updated.artifacts[0].uri, "external-action://action-1");
        assert_eq!(
            updated.artifacts[0]
                .metadata
                .as_ref()
                .and_then(|row| row.get("actionID"))
                .and_then(Value::as_str),
            Some("action-1")
        );
        assert_eq!(
            state
                .get_external_action("action-1")
                .await
                .and_then(|row| row.capability_id),
            Some("github.create_issue".to_string())
        );
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

        let objective = crate::app::routines::routine_objective_from_args(&run).expect("objective");
        let prompt = crate::app::routines::build_routine_mission_prompt(&run, &objective);

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

        let objective = crate::app::routines::routine_objective_from_args(&run).expect("objective");
        let prompt = crate::app::routines::build_routine_mission_prompt(&run, &objective);

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

    #[test]
    fn automation_blocked_nodes_respects_barrier_open_phase() {
        let automation = test_phase_automation(
            json!([
                { "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "barrier" },
                { "phase_id": "phase_2", "title": "Phase 2", "execution_mode": "soft" }
            ]),
            vec![
                test_automation_node("draft", Vec::new(), "phase_1", 1),
                test_automation_node("publish", Vec::new(), "phase_2", 100),
            ],
        );
        let run = test_phase_run(vec!["draft", "publish"], Vec::new());

        assert_eq!(
            automation_blocked_nodes(&automation, &run),
            vec!["publish".to_string()]
        );
    }

    #[test]
    fn automation_soft_phase_prefers_current_open_phase_before_priority() {
        let automation = test_phase_automation(
            json!([
                { "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "soft" },
                { "phase_id": "phase_2", "title": "Phase 2", "execution_mode": "soft" }
            ]),
            vec![
                test_automation_node("draft", Vec::new(), "phase_1", 1),
                test_automation_node("publish", Vec::new(), "phase_2", 100),
            ],
        );
        let run = test_phase_run(vec!["draft", "publish"], Vec::new());
        let phase_rank = automation_phase_rank_map(&automation);
        let current_open_phase_rank =
            automation_current_open_phase(&automation, &run).map(|(_, rank, _)| rank);
        let draft = automation
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == "draft")
            .expect("draft node");
        let publish = automation
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == "publish")
            .expect("publish node");

        assert!(automation_blocked_nodes(&automation, &run).is_empty());
        assert!(
            automation_node_sort_key(draft, &phase_rank, current_open_phase_rank)
                < automation_node_sort_key(publish, &phase_rank, current_open_phase_rank)
        );
    }

    #[test]
    fn automation_soft_phase_limits_runnable_frontier_to_current_open_phase() {
        let automation = test_phase_automation(
            json!([
                { "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "soft" },
                { "phase_id": "phase_2", "title": "Phase 2", "execution_mode": "soft" }
            ]),
            vec![
                test_automation_node("draft", Vec::new(), "phase_1", 1),
                test_automation_node("publish", Vec::new(), "phase_2", 100),
            ],
        );
        let run = test_phase_run(vec!["draft", "publish"], Vec::new());

        let filtered = automation_filter_runnable_by_open_phase(
            &automation,
            &run,
            automation.flow.nodes.clone(),
        );

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].node_id, "draft");
    }

    #[test]
    fn runnable_write_scope_filter_skips_overlapping_code_nodes() {
        let first = AutomationFlowNode {
            node_id: "first".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "First".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "write_scope": "src"
                }
            })),
        };
        let overlapping = AutomationFlowNode {
            node_id: "overlap".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Overlap".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "write_scope": "src/lib"
                }
            })),
        };
        let disjoint = AutomationFlowNode {
            node_id: "disjoint".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Disjoint".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "write_scope": "docs"
                }
            })),
        };

        let filtered = automation_filter_runnable_by_write_scope_conflicts(
            vec![first.clone(), overlapping, disjoint.clone()],
            3,
        );

        let ids = filtered
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["first", "disjoint"]);
    }

    #[test]
    fn runnable_write_scope_filter_allows_non_code_nodes_to_run_in_parallel() {
        let code = AutomationFlowNode {
            node_id: "code".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Code".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "write_scope": "src"
                }
            })),
        };
        let brief = AutomationFlowNode {
            node_id: "brief".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Brief".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md"
                }
            })),
        };

        let filtered = automation_filter_runnable_by_write_scope_conflicts(
            vec![code.clone(), brief.clone()],
            2,
        );

        let ids = filtered
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["code", "brief"]);
    }

    #[test]
    fn output_validator_defaults_follow_existing_runtime_heuristics() {
        let code = AutomationFlowNode {
            node_id: "code".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Implement fix".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "output_path": "src/lib.rs"
                }
            })),
        };
        let brief = AutomationFlowNode {
            node_id: "brief".to_string(),
            agent_id: "agent-b".to_string(),
            objective: "Draft research brief".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: None,
        };
        let review = AutomationFlowNode {
            node_id: "review".to_string(),
            agent_id: "agent-c".to_string(),
            objective: "Approve draft".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "review".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Review),
            gate: None,
            metadata: None,
        };

        assert_eq!(
            automation_output_validator_kind(&code),
            crate::AutomationOutputValidatorKind::CodePatch
        );
        assert_eq!(
            automation_output_validator_kind(&brief),
            crate::AutomationOutputValidatorKind::ResearchBrief
        );
        assert_eq!(
            automation_output_validator_kind(&review),
            crate::AutomationOutputValidatorKind::ReviewDecision
        );
    }

    #[test]
    fn output_validator_explicit_override_wins() {
        let node = AutomationFlowNode {
            node_id: "report".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Write report".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: None,
        };

        assert_eq!(
            automation_output_validator_kind(&node),
            crate::AutomationOutputValidatorKind::StructuredJson
        );
    }

    #[test]
    fn enrich_automation_node_output_overwrites_stale_validator_metadata() {
        let node = AutomationFlowNode {
            node_id: "brief".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Draft research brief".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: None,
        };
        let output = json!({
            "node_id": "brief",
            "status": "blocked",
            "workflow_class": "artifact",
            "phase": "completed",
            "failure_kind": "verification_failed",
            "validator_kind": "generic_artifact",
            "validator_summary": {
                "kind": "generic_artifact",
                "outcome": "passed"
            },
            "artifact_validation": {
                "unmet_requirements": ["concrete_read_required"]
            }
        });

        let enriched = enrich_automation_node_output_for_contract(&node, &output);
        assert_eq!(
            enriched.get("validator_kind").and_then(Value::as_str),
            Some("research_brief")
        );
        assert_eq!(
            enriched.get("workflow_class").and_then(Value::as_str),
            Some("research")
        );
        assert_eq!(
            enriched.get("phase").and_then(Value::as_str),
            Some("research_validation")
        );
        assert_eq!(
            enriched.get("failure_kind").and_then(Value::as_str),
            Some("research_missing_reads")
        );
        assert_eq!(
            enriched
                .get("validator_summary")
                .and_then(|value| value.get("outcome"))
                .and_then(Value::as_str),
            Some("blocked")
        );
    }

    #[test]
    fn placeholder_artifact_text_is_rejected() {
        assert!(placeholder_like_artifact_text(
            "Completed previously in this run; preserving file creation requirement."
        ));
        assert!(placeholder_like_artifact_text(
            "Created/updated to satisfy workflow artifact requirement. See existing workspace research already completed in this run."
        ));
        assert!(placeholder_like_artifact_text(
            "Marketing brief completed and written to marketing-brief.md."
        ));
        assert!(placeholder_like_artifact_text(
            "Marketing brief already written in prior step; no content change."
        ));
        assert!(placeholder_like_artifact_text(
            "# Status\n\nBlocked handoff"
        ));
        assert!(!placeholder_like_artifact_text(
            "# Marketing Brief\n\n## Audience\nReal sourced content with specific product details."
        ));
    }

    #[test]
    fn artifact_validation_rejection_blocks_node_status() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let tool_telemetry = json!({
            "requested_tools": ["glob", "read", "write", "websearch"],
            "executed_tools": ["glob", "write"],
            "workspace_inspection_used": true,
            "web_research_used": false
        });
        let artifact_validation = json!({
            "accepted_artifact_path": Value::Null,
            "rejected_artifact_reason": "placeholder overwrite rejected",
            "undeclared_files_created": ["_automation_touch.txt"],
            "auto_cleaned": true,
            "execution_policy": {
                "mode": "filesystem_standard"
            }
        });

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done",
            None,
            &tool_telemetry,
            Some(&artifact_validation),
        );

        assert_eq!(status, "blocked");
        assert_eq!(reason.as_deref(), Some("placeholder overwrite rejected"));
        assert_eq!(approved, None);
    }

    #[test]
    fn research_workflow_failure_kind_is_typed_from_unmet_requirements() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let artifact_validation = json!({
            "semantic_block_reason": "research completed without concrete file reads or required source coverage",
            "unmet_requirements": ["no_concrete_reads", "files_reviewed_not_backed_by_read"],
            "verification": {
                "verification_failed": false
            }
        });

        assert_eq!(
            detect_automation_node_failure_kind(
                &node,
                "blocked",
                None,
                Some("research completed without concrete file reads or required source coverage"),
                Some(&artifact_validation),
            )
            .as_deref(),
            Some("research_missing_reads")
        );
        assert_eq!(
            detect_automation_node_phase(&node, "blocked", Some(&artifact_validation)),
            "research_validation"
        );
        let summary = build_automation_validator_summary(
            crate::AutomationOutputValidatorKind::ResearchBrief,
            "blocked",
            Some("research completed without concrete file reads or required source coverage"),
            Some(&artifact_validation),
        );
        assert_eq!(
            summary.kind,
            crate::AutomationOutputValidatorKind::ResearchBrief
        );
        assert_eq!(summary.outcome, "blocked");
        assert_eq!(
            summary.reason.as_deref(),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(
            summary.unmet_requirements,
            vec![
                "no_concrete_reads".to_string(),
                "files_reviewed_not_backed_by_read".to_string()
            ]
        );
    }

    #[test]
    fn research_workflow_status_is_needs_repair_before_repair_budget_is_exhausted() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let tool_telemetry = json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "write"],
        });
        let artifact_validation = json!({
            "semantic_block_reason": "research completed without concrete file reads or required source coverage",
            "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
            "repair_exhausted": false,
        });

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written.",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

        assert_eq!(status, "needs_repair");
        assert_eq!(
            reason.as_deref(),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(approved, None);
        let summary = build_automation_validator_summary(
            crate::AutomationOutputValidatorKind::ResearchBrief,
            &status,
            reason.as_deref(),
            Some(&artifact_validation),
        );
        assert_eq!(summary.outcome, "needs_repair");
    }

    #[test]
    fn research_workflow_status_blocks_after_repair_budget_is_exhausted() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let tool_telemetry = json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "write"],
        });
        let artifact_validation = json!({
            "semantic_block_reason": "research completed without concrete file reads or required source coverage",
            "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
            "repair_exhausted": true,
        });

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written.",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

        assert_eq!(status, "blocked");
        assert_eq!(
            detect_automation_node_failure_kind(
                &node,
                &status,
                approved,
                reason.as_deref(),
                Some(&artifact_validation),
            )
            .as_deref(),
            Some("research_retry_exhausted")
        );
    }

    #[test]
    fn research_workflow_status_ignores_llm_blocked_when_validation_is_repairable() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let tool_telemetry = json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "write"],
        });
        let artifact_validation = json!({
            "semantic_block_reason": "research completed without concrete file reads or required source coverage",
            "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
            "repair_exhausted": false,
        });

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

        assert_eq!(status, "needs_repair");
        assert_eq!(
            reason.as_deref(),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(approved, None);
    }

    #[test]
    fn research_workflow_status_keeps_blocked_when_repair_is_exhausted_even_if_llm_declares_blocked(
    ) {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let tool_telemetry = json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "write"],
        });
        let artifact_validation = json!({
            "semantic_block_reason": "research completed without concrete file reads or required source coverage",
            "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
            "repair_exhausted": true,
        });

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

        assert_eq!(status, "blocked");
        assert_eq!(reason.as_deref(), Some("tools unavailable"));
        assert_eq!(approved, None);
    }

    #[test]
    fn render_automation_repair_brief_summarizes_previous_research_miss() {
        let node = AutomationFlowNode {
            node_id: "research-brief".to_string(),
            agent_id: "research".to_string(),
            objective: "Write marketing-brief.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let prior_output = json!({
            "status": "needs_repair",
            "validator_summary": {
                "reason": "research completed without required current web research",
                "unmet_requirements": [
                    "missing_successful_web_research",
                    "web_sources_reviewed_missing"
                ]
            },
            "tool_telemetry": {
                "requested_tools": ["glob", "read", "websearch", "write"],
                "executed_tools": ["glob", "write"]
            },
            "artifact_validation": {
                "blocking_classification": "tool_available_but_not_used",
                "unreviewed_relevant_paths": ["docs/pricing.md", "docs/customers.md"],
                "repair_attempt": 1,
                "repair_attempts_remaining": 4,
                "required_next_tool_actions": [
                    "Use `read` on the remaining relevant workspace files: docs/pricing.md, docs/customers.md.",
                    "Use `websearch` successfully and include the resulting sources in `Web sources reviewed`."
                ]
            }
        });

        let brief =
            render_automation_repair_brief(&node, Some(&prior_output), 2, 5).expect("repair brief");

        assert!(brief.contains("needs_repair"));
        assert!(brief.contains("missing_successful_web_research"));
        assert!(brief.contains("tool_available_but_not_used"));
        assert!(brief.contains("Required next tool actions"));
        assert!(brief.contains("Use `read` on the remaining relevant workspace files"));
        assert!(brief.contains("glob, read, websearch, write"));
        assert!(brief.contains("glob, write"));
        assert!(brief.contains("docs/pricing.md, docs/customers.md"));
        assert!(brief.contains("Remaining repair attempts after this run: 3"));
    }

    #[test]
    fn research_nodes_default_to_five_attempts() {
        let node = AutomationFlowNode {
            node_id: "research-brief".to_string(),
            agent_id: "research".to_string(),
            objective: "Write marketing-brief.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: None,
        };

        assert_eq!(automation_node_max_attempts(&node), 5);
    }

    #[test]
    fn first_attempt_research_prompt_requires_completed_status() {
        let automation = AutomationV2Spec {
            automation_id: "automation-1".to_string(),
            name: "Research Automation".to_string(),
            description: None,
            status: crate::AutomationV2Status::Active,
            schedule: crate::AutomationV2Schedule {
                schedule_type: crate::AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            agents: Vec::new(),
            flow: crate::AutomationFlowSpec { nodes: Vec::new() },
            execution: crate::AutomationExecutionPolicy {
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 0,
            updated_at_ms: 0,
            creator_id: "test".to_string(),
            workspace_root: Some("/tmp".to_string()),
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };
        let node = AutomationFlowNode {
            node_id: "research-brief".to_string(),
            agent_id: "research".to_string(),
            objective: "Write marketing-brief.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "source_coverage_required": true,
                    "web_research_expected": true
                }
            })),
        };
        let agent = AutomationAgentProfile {
            agent_id: "research".to_string(),
            template_id: None,
            display_name: "Research".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec![
                    "glob".to_string(),
                    "read".to_string(),
                    "websearch".to_string(),
                    "write".to_string(),
                ],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        };

        let prompt = render_automation_v2_prompt(
            &automation,
            "/tmp",
            "run-1",
            &node,
            1,
            &agent,
            &[],
            None,
            None,
            None,
        );

        assert!(prompt.contains("`status` set to `completed`"));
        assert!(prompt.contains("Do not declare the output blocked"));
        assert!(!prompt.contains("at least `status` (`completed` or `blocked`)"));
    }

    #[test]
    fn automation_node_required_tools_reads_builder_metadata() {
        let node = AutomationFlowNode {
            node_id: "artifact".to_string(),
            agent_id: "writer".to_string(),
            objective: "Write notes.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "artifact".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "notes.md",
                    "required_tools": ["read", "websearch"]
                }
            })),
        };

        assert_eq!(
            automation_node_required_tools(&node),
            vec!["read".to_string(), "websearch".to_string()]
        );
    }

    #[test]
    fn generic_required_tools_prewrite_requirements_enable_repair() {
        let node = AutomationFlowNode {
            node_id: "artifact".to_string(),
            agent_id: "writer".to_string(),
            objective: "Write notes.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "artifact".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "notes.md",
                    "web_research_expected": true,
                    "required_tools": ["read", "websearch"]
                }
            })),
        };

        let requirements = automation_node_prewrite_requirements(
            &node,
            &[
                "read".to_string(),
                "websearch".to_string(),
                "write".to_string(),
            ],
        )
        .expect("prewrite requirements");

        assert!(requirements.concrete_read_required);
        assert!(requirements.successful_web_research_required);
        assert!(requirements.repair_on_unmet_requirements);
        assert_eq!(requirements.coverage_mode, PrewriteCoverageMode::None);
    }

    #[test]
    fn generic_required_tools_validation_needs_repair_when_read_unused() {
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-required-tools-test-{}", now_ms()));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");

        let node = AutomationFlowNode {
            node_id: "artifact".to_string(),
            agent_id: "writer".to_string(),
            objective: "Write notes.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "artifact".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "notes.md",
                    "required_tools": ["read"]
                }
            })),
        };
        let mut session = Session::new(Some("required tools".to_string()), None);
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path":"notes.md",
                    "content":"# Notes\n\nA short summary written without reading sources.\n"
                }),
                result: Some(json!({"output":"written"})),
                error: None,
            }],
        ));

        let tool_telemetry = summarize_automation_tool_activity(
            &node,
            &session,
            &["read".to_string(), "write".to_string()],
        );
        let (_, artifact_validation, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            "",
            &tool_telemetry,
            None,
            Some((
                "notes.md".to_string(),
                "# Notes\n\nA short summary written without reading sources.\n".to_string(),
            )),
            &std::collections::BTreeSet::new(),
        );

        assert!(rejected.is_some());
        assert_eq!(
            artifact_validation
                .get("semantic_block_reason")
                .and_then(Value::as_str),
            Some("artifact finalized without using required tools")
        );
        assert_eq!(
            artifact_validation
                .get("validation_outcome")
                .and_then(Value::as_str),
            Some("needs_repair")
        );
        assert_eq!(
            artifact_validation
                .get("blocking_classification")
                .and_then(Value::as_str),
            Some("tool_available_but_not_used")
        );
        assert_eq!(
            artifact_validation
                .get("required_next_tool_actions")
                .and_then(Value::as_array)
                .and_then(|rows| rows.first())
                .and_then(Value::as_str),
            Some("Use `read` on concrete workspace files before finalizing the brief.")
        );

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done — `notes.md` was written.",
            Some(&(
                "notes.md".to_string(),
                "# Notes\n\nA short summary written without reading sources.\n".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );
        assert_eq!(status, "needs_repair");
        assert_eq!(
            detect_automation_node_failure_kind(
                &node,
                &status,
                approved,
                reason.as_deref(),
                Some(&artifact_validation),
            )
            .as_deref(),
            Some("required_tool_unused_read")
        );

        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn generic_required_tools_nodes_default_to_five_attempts() {
        let node = AutomationFlowNode {
            node_id: "artifact".to_string(),
            agent_id: "writer".to_string(),
            objective: "Write notes.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "artifact".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "notes.md",
                    "required_tools": ["read"]
                }
            })),
        };

        assert_eq!(automation_node_max_attempts(&node), 5);
    }

    #[test]
    fn first_attempt_required_tools_prompt_requires_completed_status() {
        let automation = AutomationV2Spec {
            automation_id: "automation-2".to_string(),
            name: "Generic Artifact Automation".to_string(),
            description: None,
            status: crate::AutomationV2Status::Active,
            schedule: crate::AutomationV2Schedule {
                schedule_type: crate::AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            agents: Vec::new(),
            flow: crate::AutomationFlowSpec { nodes: Vec::new() },
            execution: crate::AutomationExecutionPolicy {
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 0,
            updated_at_ms: 0,
            creator_id: "test".to_string(),
            workspace_root: Some("/tmp".to_string()),
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };
        let node = AutomationFlowNode {
            node_id: "artifact".to_string(),
            agent_id: "writer".to_string(),
            objective: "Write notes.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "artifact".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "notes.md",
                    "required_tools": ["read"]
                }
            })),
        };
        let agent = AutomationAgentProfile {
            agent_id: "writer".to_string(),
            template_id: None,
            display_name: "Writer".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["read".to_string(), "write".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        };

        let prompt = render_automation_v2_prompt(
            &automation,
            "/tmp",
            "run-2",
            &node,
            1,
            &agent,
            &[],
            None,
            None,
            None,
        );

        assert!(prompt.contains("`status` set to `completed`"));
        assert!(prompt.contains("required workflow tools remain available"));
        assert!(!prompt.contains("at least `status` (`completed` or `blocked`)"));
    }

    #[test]
    fn research_required_next_tool_actions_summarize_missing_reads_and_websearch() {
        let requested_tools = vec![
            json!("glob"),
            json!("read"),
            json!("websearch"),
            json!("write"),
        ];
        let executed_tools = vec![json!("glob"), json!("write")];
        let unmet_requirements = vec![
            "no_concrete_reads".to_string(),
            "missing_successful_web_research".to_string(),
            "web_sources_reviewed_missing".to_string(),
            "relevant_files_not_reviewed_or_skipped".to_string(),
        ];
        let unreviewed_relevant_paths = vec![
            "docs/pricing.md".to_string(),
            "docs/customers.md".to_string(),
        ];

        let actions = research_required_next_tool_actions(
            &requested_tools,
            &executed_tools,
            true,
            &unmet_requirements,
            &unreviewed_relevant_paths,
        );

        assert!(actions
            .iter()
            .any(|value| value.contains("docs/pricing.md, docs/customers.md")));
        assert!(actions
            .iter()
            .any(|value| value.contains("Use `websearch` successfully")));
        assert!(actions
            .iter()
            .any(|value| value.contains("Files not reviewed")));
    }

    #[test]
    fn research_workflow_failure_kind_detects_missing_citations() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let artifact_validation = json!({
            "semantic_block_reason": "research completed without citation-backed claims",
            "unmet_requirements": ["citations_missing", "web_sources_reviewed_missing"],
            "verification": {
                "verification_failed": false
            }
        });

        assert_eq!(
            detect_automation_node_failure_kind(
                &node,
                "blocked",
                None,
                Some("research completed without citation-backed claims"),
                Some(&artifact_validation),
            )
            .as_deref(),
            Some("research_citations_missing")
        );
        assert_eq!(
            detect_automation_node_phase(&node, "blocked", Some(&artifact_validation)),
            "research_validation"
        );
    }

    #[test]
    fn research_workflow_defaults_to_warning_without_strict_source_coverage() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true
                }
            })),
        };
        let artifact_validation = json!({
            "unmet_requirements": ["no_concrete_reads", "citations_missing", "missing_successful_web_research"],
            "verification": {
                "verification_failed": false
            }
        });

        assert_eq!(
            detect_automation_node_failure_kind(
                &node,
                "completed",
                None,
                None,
                Some(&artifact_validation)
            ),
            None
        );
        assert_eq!(
            detect_automation_node_phase(&node, "completed", Some(&artifact_validation)),
            "completed"
        );
    }

    #[test]
    fn validator_summary_reports_repair_attempt_state() {
        let artifact_validation = json!({
            "semantic_block_reason": "research completed without citation-backed claims",
            "unmet_requirements": ["citations_missing"],
            "repair_attempted": true,
            "repair_attempt": 2,
            "repair_attempts_remaining": 0,
            "repair_succeeded": false,
            "repair_exhausted": true,
        });
        let summary = build_automation_validator_summary(
            crate::AutomationOutputValidatorKind::ResearchBrief,
            "blocked",
            Some("research completed without citation-backed claims"),
            Some(&artifact_validation),
        );
        assert!(summary.repair_attempted);
        assert_eq!(summary.repair_attempt, 2);
        assert_eq!(summary.repair_attempts_remaining, 0);
        assert!(!summary.repair_succeeded);
        assert!(summary.repair_exhausted);
    }

    #[test]
    fn artifact_validation_uses_structured_repair_exhaustion_state_from_session_text() {
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-repair-state-test-{}", now_ms()));
        std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
        std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
            .expect("seed input file");

        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let mut session = Session::new(Some("research repair exhausted".to_string()), None);
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path":"marketing-brief.md",
                    "content":"# Marketing Brief\n\n## Findings\nBlocked draft without citations.\n"
                }),
                result: Some(json!({"output":"written"})),
                error: None,
            }],
        ));
        let tool_telemetry = summarize_automation_tool_activity(
            &node,
            &session,
            &[
                "glob".to_string(),
                "read".to_string(),
                "websearch".to_string(),
                "write".to_string(),
            ],
        );
        let session_text = r#"TOOL_MODE_REQUIRED_NOT_SATISFIED: PREWRITE_REQUIREMENTS_EXHAUSTED

{"status":"blocked","reason":"prewrite requirements exhausted before final artifact validation","failureCode":"PREWRITE_REQUIREMENTS_EXHAUSTED","repairAttempt":2,"repairAttemptsRemaining":0,"repairExhausted":true,"unmetRequirements":["concrete_read_required","successful_web_research_required"]}"#;
        let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            session_text,
            &tool_telemetry,
            None,
            Some((
                "marketing-brief.md".to_string(),
                "# Marketing Brief\n\n## Findings\nBlocked draft without citations.\n".to_string(),
            )),
            &std::collections::BTreeSet::new(),
        );
        assert!(rejected.is_some());
        assert_eq!(
            metadata.get("repair_attempt").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            metadata
                .get("repair_attempts_remaining")
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            metadata.get("repair_exhausted").and_then(Value::as_bool),
            Some(true)
        );
        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn research_artifact_validation_requires_citations_and_web_sources_reviewed() {
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-research-citation-test-{}", now_ms()));
        std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
        std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
            .expect("seed input file");

        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true,
                    "source_coverage_required": true
                }
            })),
        };
        let mut session = Session::new(Some("research citations".to_string()), None);
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "read".to_string(),
                    args: json!({"path":"inputs/questions.md"}),
                    result: Some(json!({"output":"Question"})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "websearch".to_string(),
                    args: json!({"query":"market trends"}),
                    result: Some(json!({"output":"Search results found"})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path":"marketing-brief.md",
                        "content":"# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Files not reviewed\n- inputs/references.md: not available in this run.\n\n## Findings\nClaims are summarized here without explicit citations.\n"
                    }),
                    result: Some(json!({"output":"written"})),
                    error: None,
                },
            ],
        ));

        let tool_telemetry = summarize_automation_tool_activity(
            &node,
            &session,
            &[
                "read".to_string(),
                "write".to_string(),
                "websearch".to_string(),
            ],
        );
        let (_, artifact_validation, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            "",
            &tool_telemetry,
            None,
            Some((
                "marketing-brief.md".to_string(),
                "# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Findings\nClaims are summarized here without explicit citations.\n".to_string(),
            )),
            &std::collections::BTreeSet::new(),
        );

        assert_eq!(
            rejected.as_deref(),
            Some("research completed without citation-backed claims")
        );
        assert_eq!(
            artifact_validation
                .get("unmet_requirements")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            vec![
                json!("citations_missing"),
                json!("web_sources_reviewed_missing")
            ]
        );
        assert_eq!(
            artifact_validation
                .get("artifact_candidates")
                .and_then(Value::as_array)
                .and_then(|rows| rows.first())
                .and_then(|value| value.get("citation_count"))
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            artifact_validation
                .get("citation_count")
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            artifact_validation
                .get("web_sources_reviewed_present")
                .and_then(Value::as_bool),
            Some(false)
        );

        let _ = std::fs::remove_dir_all(&workspace_root);
    }

    #[test]
    fn validator_summary_tracks_verification_and_repair_state() {
        let artifact_validation = json!({
            "accepted_candidate_source": "session_write",
            "repair_attempted": true,
            "repair_succeeded": true,
            "verification": {
                "verification_outcome": "passed"
            }
        });

        let summary = build_automation_validator_summary(
            crate::AutomationOutputValidatorKind::CodePatch,
            "done",
            None,
            Some(&artifact_validation),
        );

        assert_eq!(
            summary.kind,
            crate::AutomationOutputValidatorKind::CodePatch
        );
        assert_eq!(summary.outcome, "passed");
        assert_eq!(
            summary.accepted_candidate_source.as_deref(),
            Some("session_write")
        );
        assert_eq!(summary.verification_outcome.as_deref(), Some("passed"));
        assert!(summary.repair_attempted);
        assert!(summary.repair_succeeded);
    }

    #[test]
    fn generic_artifact_validation_blocks_weak_report_markdown() {
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-editorial-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace_root).expect("workspace dir");
        let node = AutomationFlowNode {
            node_id: "draft-report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Draft the final report".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "report.md"
                }
            })),
        };
        let session = Session::new(
            Some("editorial".to_string()),
            Some(workspace_root.to_string_lossy().to_string()),
        );
        let tool_telemetry = json!({
            "requested_tools": ["write"],
            "executed_tools": ["write"],
            "tool_call_counts": {
                "write": 1
            }
        });
        let (_, artifact_validation, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            "",
            &tool_telemetry,
            None,
            Some(("report.md".to_string(), "# Draft\n\nTODO\n".to_string())),
            &std::collections::BTreeSet::new(),
        );

        assert_eq!(
            rejected.as_deref(),
            Some("editorial artifact is missing expected markdown structure")
        );
        assert_eq!(
            artifact_validation
                .get("unmet_requirements")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            vec![
                json!("editorial_substance_missing"),
                json!("markdown_structure_missing")
            ]
        );
        assert_eq!(
            artifact_validation
                .get("heading_count")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            artifact_validation
                .get("paragraph_count")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            detect_automation_node_failure_kind(
                &node,
                "blocked",
                None,
                None,
                Some(&artifact_validation),
            ),
            Some("editorial_quality_failed".to_string())
        );
        assert_eq!(
            detect_automation_node_phase(&node, "blocked", Some(&artifact_validation)),
            "editorial_validation"
        );

        let _ = std::fs::remove_dir_all(&workspace_root);
    }

    #[test]
    fn publish_node_blocks_when_upstream_editorial_validation_failed() {
        let publish = AutomationFlowNode {
            node_id: "publish".to_string(),
            agent_id: "publisher".to_string(),
            objective: "Publish final output".to_string(),
            depends_on: vec!["draft".to_string()],
            input_refs: vec![AutomationFlowInputRef {
                from_step_id: "draft".to_string(),
                alias: "draft".to_string(),
            }],
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "role": "publisher"
                }
            })),
        };
        let mut run = test_phase_run(vec!["publish"], vec!["draft"]);
        run.checkpoint.node_outputs.insert(
            "draft".to_string(),
            json!({
                "node_id": "draft",
                "failure_kind": "editorial_quality_failed",
                "phase": "editorial_validation",
                "validator_summary": {
                    "unmet_requirements": ["editorial_substance_missing", "markdown_structure_missing"]
                }
            }),
        );

        let reason =
            automation_publish_editorial_block_reason(&run, &publish).expect("publish block");
        assert!(reason.contains("draft"));
        assert!(reason.contains("editorial"));
    }

    #[test]
    fn execution_policy_reports_workflow_class() {
        let research = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md"
                }
            })),
        };
        let code = AutomationFlowNode {
            node_id: "code".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Code".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "output_path": "handoff.md"
                }
            })),
        };

        assert_eq!(
            automation_node_execution_policy(&research, ".")
                .get("workflow_class")
                .and_then(Value::as_str),
            Some("research")
        );
        assert_eq!(
            automation_node_execution_policy(&code, ".")
                .get("workflow_class")
                .and_then(Value::as_str),
            Some("code")
        );
    }

    #[test]
    fn workflow_state_events_capture_typed_stability_transitions() {
        let mut run = AutomationV2RunRecord {
            run_id: "run-1".to_string(),
            automation_id: "automation-1".to_string(),
            trigger_type: "manual".to_string(),
            status: AutomationRunStatus::Running,
            created_at_ms: 0,
            updated_at_ms: 0,
            started_at_ms: Some(0),
            finished_at_ms: None,
            latest_session_id: None,
            active_session_ids: Vec::new(),
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes: Vec::new(),
                node_outputs: std::collections::HashMap::new(),
                node_attempts: std::collections::HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            automation_snapshot: None,
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
        let output = json!({
            "status": "blocked",
            "workflow_class": "research",
            "phase": "research_validation",
            "failure_kind": "research_missing_reads",
            "blocked_reason": "research completed without concrete file reads",
            "artifact_validation": {
                "accepted_candidate_source": "session_write_recovery",
                "artifact_candidates": [
                    {
                        "source": "session_write",
                        "length": 1200,
                        "substantive": true,
                        "placeholder_like": false,
                        "accepted": false
                    }
                ],
                "repair_attempted": true,
                "repair_succeeded": false,
                "unmet_requirements": ["no_concrete_reads"],
                "blocking_classification": "tool_available_but_not_used",
                "required_next_tool_actions": [
                    "Use `read` on concrete workspace files before finalizing the brief."
                ],
                "verification": {
                    "verification_expected": false,
                    "verification_ran": false,
                    "verification_failed": false
                }
            }
        });

        record_automation_workflow_state_events(
            &mut run,
            "research-brief",
            &output,
            2,
            Some("session-1"),
            "blocked brief",
            "brief",
        );

        let events = run
            .checkpoint
            .lifecycle_history
            .iter()
            .map(|event| event.event.as_str())
            .collect::<Vec<_>>();
        assert!(events.contains(&"workflow_state_changed"));
        assert!(events.contains(&"artifact_candidate_written"));
        assert!(events.contains(&"artifact_accepted"));
        assert!(events.contains(&"repair_started"));
        assert!(events.contains(&"repair_exhausted"));
        assert!(events.contains(&"research_coverage_failed"));

        let state_event = run
            .checkpoint
            .lifecycle_history
            .iter()
            .find(|event| event.event == "workflow_state_changed")
            .expect("workflow state event");
        assert_eq!(
            state_event
                .metadata
                .as_ref()
                .and_then(|value| value.get("workflow_class"))
                .and_then(Value::as_str),
            Some("research")
        );
        assert_eq!(
            state_event
                .metadata
                .as_ref()
                .and_then(|value| value.get("failure_kind"))
                .and_then(Value::as_str),
            Some("research_missing_reads")
        );
        assert_eq!(
            state_event
                .metadata
                .as_ref()
                .and_then(|value| value.get("blocking_classification"))
                .and_then(Value::as_str),
            Some("tool_available_but_not_used")
        );
        assert_eq!(
            state_event
                .metadata
                .as_ref()
                .and_then(|value| value.get("required_next_tool_actions"))
                .and_then(Value::as_array)
                .and_then(|rows| rows.first())
                .and_then(Value::as_str),
            Some("Use `read` on concrete workspace files before finalizing the brief.")
        );
    }

    #[test]
    fn code_workflow_verification_failure_sets_verify_failed_status() {
        let node = AutomationFlowNode {
            node_id: "implement".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Implement feature".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "verification_command": "cargo test"
                }
            })),
        };
        let tool_telemetry = json!({
            "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
            "executed_tools": ["read", "apply_patch", "bash"],
            "verification_expected": true,
            "verification_ran": true,
            "verification_failed": true,
            "latest_verification_failure": "verification command failed with exit code 101: cargo test"
        });

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            None,
            &tool_telemetry,
            None,
        );

        assert_eq!(status, "verify_failed");
        assert_eq!(
            reason.as_deref(),
            Some("verification command failed with exit code 101: cargo test")
        );
        assert_eq!(approved, None);
    }

    #[test]
    fn code_workflow_without_verification_run_is_blocked() {
        let node = AutomationFlowNode {
            node_id: "implement".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Implement feature".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "verification_command": "cargo test"
                }
            })),
        };
        let tool_telemetry = json!({
            "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
            "executed_tools": ["read", "apply_patch"],
            "verification_expected": true,
            "verification_ran": false,
            "verification_failed": false
        });

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            None,
            &tool_telemetry,
            None,
        );

        assert_eq!(status, "blocked");
        assert_eq!(
            reason.as_deref(),
            Some("coding task completed without running the declared verification command")
        );
        assert_eq!(approved, None);
    }

    #[test]
    fn collect_automation_external_action_receipts_records_bound_publisher_tools() {
        let automation = AutomationV2Spec {
            automation_id: "auto-publish-test".to_string(),
            name: "Publish Test".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
            },
            agents: Vec::new(),
            flow: AutomationFlowSpec { nodes: Vec::new() },
            execution: AutomationExecutionPolicy {
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: "test".to_string(),
            workspace_root: Some(".".to_string()),
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };
        let node = AutomationFlowNode {
            node_id: "publish".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Publish final update".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "role": "publisher"
                }
            })),
        };
        let mut session = Session::new(Some("publisher".to_string()), Some(".".to_string()));
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "workflow_test.slack".to_string(),
                    args: json!({
                        "channel": "engineering",
                        "text": "Ship it"
                    }),
                    result: Some(json!({
                        "output": "posted",
                        "metadata": {
                            "channel": "engineering"
                        }
                    })),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "workflow_test.internal".to_string(),
                    args: json!({
                        "value": 1
                    }),
                    result: Some(json!({"output": "ignored"})),
                    error: None,
                },
            ],
        ));
        let mut bindings = capability_resolver::CapabilityBindingsFile::default();
        bindings
            .bindings
            .push(capability_resolver::CapabilityBinding {
                capability_id: "slack.post_message".to_string(),
                provider: "custom".to_string(),
                tool_name: "workflow_test.slack".to_string(),
                tool_name_aliases: Vec::new(),
                request_transform: None,
                response_transform: None,
                metadata: json!({}),
            });

        let receipts = collect_automation_external_action_receipts(
            &bindings,
            "run-1",
            &automation,
            &node,
            1,
            "session-1",
            &session,
        );

        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].source_kind.as_deref(), Some("automation_v2"));
        assert_eq!(
            receipts[0].capability_id.as_deref(),
            Some("slack.post_message")
        );
        assert_eq!(
            receipts[0].context_run_id.as_deref(),
            Some("automation-v2-run-1")
        );
        assert_eq!(receipts[0].target.as_deref(), Some("engineering"));
    }

    #[test]
    fn collect_automation_external_action_receipts_ignores_non_outbound_nodes() {
        let automation = AutomationV2Spec {
            automation_id: "auto-draft-test".to_string(),
            name: "Draft Test".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
            },
            agents: Vec::new(),
            flow: AutomationFlowSpec { nodes: Vec::new() },
            execution: AutomationExecutionPolicy {
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: "test".to_string(),
            workspace_root: Some(".".to_string()),
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };
        let node = AutomationFlowNode {
            node_id: "draft".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Draft final update".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "role": "writer"
                }
            })),
        };
        let mut session = Session::new(Some("writer".to_string()), Some(".".to_string()));
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "workflow_test.slack".to_string(),
                args: json!({
                    "channel": "engineering",
                    "text": "Ship it"
                }),
                result: Some(json!({"output": "posted"})),
                error: None,
            }],
        ));
        let mut bindings = capability_resolver::CapabilityBindingsFile::default();
        bindings
            .bindings
            .push(capability_resolver::CapabilityBinding {
                capability_id: "slack.post_message".to_string(),
                provider: "custom".to_string(),
                tool_name: "workflow_test.slack".to_string(),
                tool_name_aliases: Vec::new(),
                request_transform: None,
                response_transform: None,
                metadata: json!({}),
            });

        let receipts = collect_automation_external_action_receipts(
            &bindings,
            "run-1",
            &automation,
            &node,
            1,
            "session-1",
            &session,
        );

        assert!(receipts.is_empty());
    }

    #[test]
    fn collect_automation_external_action_receipts_include_attempt_in_identity() {
        let automation = AutomationV2Spec {
            automation_id: "auto-publish-attempt-test".to_string(),
            name: "Publish Attempt Test".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
            },
            agents: Vec::new(),
            flow: AutomationFlowSpec { nodes: Vec::new() },
            execution: AutomationExecutionPolicy {
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: "test".to_string(),
            workspace_root: Some(".".to_string()),
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
        };
        let node = AutomationFlowNode {
            node_id: "publish".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Publish final update".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: Some(AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "role": "publisher"
                }
            })),
        };
        let mut session = Session::new(Some("publisher".to_string()), Some(".".to_string()));
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "workflow_test.slack".to_string(),
                args: json!({
                    "channel": "engineering",
                    "text": "Ship it"
                }),
                result: Some(json!({"output": "posted"})),
                error: None,
            }],
        ));
        let mut bindings = capability_resolver::CapabilityBindingsFile::default();
        bindings
            .bindings
            .push(capability_resolver::CapabilityBinding {
                capability_id: "slack.post_message".to_string(),
                provider: "custom".to_string(),
                tool_name: "workflow_test.slack".to_string(),
                tool_name_aliases: Vec::new(),
                request_transform: None,
                response_transform: None,
                metadata: json!({}),
            });

        let first_attempt = collect_automation_external_action_receipts(
            &bindings,
            "run-1",
            &automation,
            &node,
            1,
            "session-1",
            &session,
        );
        let second_attempt = collect_automation_external_action_receipts(
            &bindings,
            "run-1",
            &automation,
            &node,
            2,
            "session-1",
            &session,
        );

        assert_eq!(first_attempt.len(), 1);
        assert_eq!(second_attempt.len(), 1);
        assert_ne!(first_attempt[0].action_id, second_attempt[0].action_id);
        assert_ne!(
            first_attempt[0].idempotency_key,
            second_attempt[0].idempotency_key
        );
        assert_ne!(first_attempt[0].source_id, second_attempt[0].source_id);
    }

    #[test]
    fn code_workflow_with_full_verification_plan_reports_done() {
        let node = AutomationFlowNode {
            node_id: "implement".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Implement feature".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "verification_command": "cargo check\ncargo test\ncargo clippy --all-targets"
                }
            })),
        };
        let mut session = Session::new(Some("verification pass".to_string()), None);
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "bash".to_string(),
                    args: json!({"command":"cargo check"}),
                    result: Some(json!({"metadata":{"exit_code":0}})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "bash".to_string(),
                    args: json!({"command":"cargo test"}),
                    result: Some(json!({"metadata":{"exit_code":0}})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "bash".to_string(),
                    args: json!({"command":"cargo clippy --all-targets"}),
                    result: Some(json!({"metadata":{"exit_code":0}})),
                    error: None,
                },
            ],
        ));

        let tool_telemetry = summarize_automation_tool_activity(
            &node,
            &session,
            &[
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "apply_patch".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ],
        );

        assert_eq!(
            tool_telemetry
                .get("verification_outcome")
                .and_then(Value::as_str),
            Some("passed")
        );
        assert_eq!(
            tool_telemetry
                .get("verification_total")
                .and_then(Value::as_u64),
            Some(3)
        );
        assert_eq!(
            tool_telemetry
                .get("verification_completed")
                .and_then(Value::as_u64),
            Some(3)
        );

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            None,
            &tool_telemetry,
            None,
        );

        assert_eq!(status, "done");
        assert_eq!(reason, None);
        assert_eq!(approved, None);
    }

    #[test]
    fn code_workflow_with_partial_verification_is_blocked() {
        let node = AutomationFlowNode {
            node_id: "implement".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Implement feature".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "verification_command": "cargo check\ncargo test\ncargo clippy --all-targets"
                }
            })),
        };
        let mut session = Session::new(Some("verification partial".to_string()), None);
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "bash".to_string(),
                    args: json!({"command":"cargo check"}),
                    result: Some(json!({"metadata":{"exit_code":0}})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "bash".to_string(),
                    args: json!({"command":"cargo test"}),
                    result: Some(json!({"metadata":{"exit_code":0}})),
                    error: None,
                },
            ],
        ));

        let tool_telemetry = summarize_automation_tool_activity(
            &node,
            &session,
            &[
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "apply_patch".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ],
        );

        assert_eq!(
            tool_telemetry
                .get("verification_outcome")
                .and_then(Value::as_str),
            Some("partial")
        );

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            None,
            &tool_telemetry,
            None,
        );

        assert_eq!(status, "blocked");
        assert_eq!(
            reason.as_deref(),
            Some("coding task completed with only 2 of 3 declared verification commands run")
        );
        assert_eq!(approved, None);
    }

    #[test]
    fn session_read_paths_accepts_json_string_tool_args() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-session-read-paths-json-string-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");
        std::fs::write(workspace_root.join("src/lib.rs"), "pub fn demo() {}\n").expect("seed file");

        let mut session = Session::new(
            Some("json string read args".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!("{\"path\":\"src/lib.rs\"}"),
                result: Some(json!({"ok": true})),
                error: None,
            }],
        ));

        let paths = session_read_paths(
            &session,
            workspace_root.to_str().expect("workspace root string"),
        );

        assert_eq!(paths, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn session_write_candidates_accepts_json_string_tool_args() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-session-write-candidates-json-string-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");

        let mut session = Session::new(
            Some("json string write args".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!("{\"path\":\"brief.md\",\"content\":\"Draft body\"}"),
                result: Some(json!({"ok": true})),
                error: None,
            }],
        ));

        let candidates = session_write_candidates_for_output(
            &session,
            workspace_root.to_str().expect("workspace root string"),
            "brief.md",
        );

        assert_eq!(candidates, vec!["Draft body".to_string()]);
    }

    #[test]
    fn session_file_mutation_summary_accepts_json_string_tool_args() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-session-mutation-summary-json-string-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");

        let mut session = Session::new(
            Some("json string mutation args".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!("{\"path\":\"src/lib.rs\",\"content\":\"pub fn demo() {}\\n\"}"),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "apply_patch".to_string(),
                    args: json!("{\"patchText\":\"*** Begin Patch\\n*** Update File: src/other.rs\\n@@\\n-old\\n+new\\n*** End Patch\\n\"}"),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));

        let summary = session_file_mutation_summary(
            &session,
            workspace_root.to_str().expect("workspace root string"),
        );

        assert_eq!(
            summary
                .get("touched_files")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            vec![json!("src/lib.rs"), json!("src/other.rs")]
        );
        assert_eq!(
            summary
                .get("mutation_tool_by_file")
                .and_then(|value| value.get("src/lib.rs"))
                .cloned(),
            Some(json!(["write"]))
        );
        assert_eq!(
            summary
                .get("mutation_tool_by_file")
                .and_then(|value| value.get("src/other.rs"))
                .cloned(),
            Some(json!(["apply_patch"]))
        );
    }

    #[test]
    fn code_workflow_rejects_unsafe_raw_source_rewrites() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-automation-unsafe-write-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");
        std::fs::write(workspace_root.join("src/lib.rs"), "pub fn before() {}\n")
            .expect("seed source");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root string"),
        );
        let long_handoff = format!(
            "# Handoff\n\n{}\n",
            "Detailed implementation summary. ".repeat(20)
        );
        let node = AutomationFlowNode {
            node_id: "implement".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Implement feature".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "task_kind": "code_change",
                    "output_path": "handoff.md"
                }
            })),
        };
        let mut session = Session::new(
            Some("unsafe raw write".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path": "src/lib.rs",
                        "content": "pub fn after() {}\n"
                    }),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path": "handoff.md",
                        "content": long_handoff
                    }),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));

        let (_, metadata, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root string"),
            "",
            &json!({
                "requested_tools": ["read", "write"],
                "executed_tools": ["write"]
            }),
            None,
            Some(("handoff.md".to_string(), long_handoff)),
            &snapshot,
        );

        assert_eq!(
            rejected.as_deref(),
            Some("unsafe raw source rewrite rejected: src/lib.rs")
        );
        assert_eq!(
            metadata
                .get("rejected_artifact_reason")
                .and_then(Value::as_str),
            Some("unsafe raw source rewrite rejected: src/lib.rs")
        );

        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn artifact_validation_restores_substantive_session_write_over_short_completion_note() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-automation-restore-write-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root string"),
        );
        let substantive = format!(
            "# Marketing Brief\n\n## Workspace source audit\n{}\n",
            "Real sourced marketing brief content. ".repeat(40)
        );
        std::fs::write(
            workspace_root.join("marketing-brief.md"),
            "Marketing brief completed and written to marketing-brief.md.\n",
        )
        .expect("seed placeholder");
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true
                }
            })),
        };
        let mut session = Session::new(
            Some("restore substantive write".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path": "marketing-brief.md",
                        "content": substantive
                    }),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path": "marketing-brief.md",
                        "content": "Marketing brief completed and written to marketing-brief.md."
                    }),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));

        let (accepted_output, metadata, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root
                .to_str()
                .expect("workspace root string"),
            "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
            &json!({
                "requested_tools": ["glob", "read", "websearch", "write"],
                "executed_tools": ["glob", "websearch", "write"],
                "workspace_inspection_used": true,
                "web_research_used": true
            }),
            None,
            Some((
                "marketing-brief.md".to_string(),
                "Marketing brief completed and written to marketing-brief.md.".to_string(),
            )),
            &snapshot,
        );

        assert_eq!(
            rejected.as_deref(),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(
            metadata
                .get("recovered_from_session_write")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(accepted_output
            .as_ref()
            .is_some_and(|(_, text)| text.contains("## Workspace source audit")));
        let disk_text = std::fs::read_to_string(workspace_root.join("marketing-brief.md"))
            .expect("read restored file");
        assert!(disk_text.contains("## Workspace source audit"));
        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
            accepted_output.as_ref(),
            &json!({
                "requested_tools": ["glob", "read", "websearch", "write"],
                "executed_tools": ["glob", "websearch", "write"],
                "workspace_inspection_used": true,
                "web_research_used": true
            }),
            Some(&metadata),
        );
        assert_eq!(status, "needs_repair");
        assert_eq!(
            reason.as_deref(),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(approved, Some(true));

        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn artifact_validation_prefers_structurally_stronger_candidate_without_phrase_match() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-automation-stronger-candidate-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root string"),
        );
        let substantive = format!(
            "# Marketing Brief\n\n## Workspace source audit\n{}\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md (out of scope)\n",
            "Detailed sourced content. ".repeat(50)
        );
        let weak_final = "# Marketing Brief\n\nShort wrap-up.\n".to_string();
        std::fs::write(workspace_root.join("marketing-brief.md"), &weak_final)
            .expect("seed final weak artifact");
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": false
                }
            })),
        };
        let mut session = Session::new(
            Some("stronger candidate".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "read".to_string(),
                    args: json!({"path":"docs/source.md"}),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path": "marketing-brief.md",
                        "content": substantive
                    }),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path": "marketing-brief.md",
                        "content": weak_final
                    }),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));

        let (accepted_output, metadata, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root string"),
            "Done",
            &json!({
                "requested_tools": ["glob", "read", "write"],
                "executed_tools": ["read", "write"]
            }),
            None,
            Some((
                "marketing-brief.md".to_string(),
                "# Marketing Brief\n\nShort wrap-up.\n".to_string(),
            )),
            &snapshot,
        );

        assert_eq!(
            rejected.as_deref(),
            Some("research completed without citation-backed claims")
        );
        assert_eq!(
            metadata
                .get("accepted_candidate_source")
                .and_then(Value::as_str),
            Some("session_write")
        );
        assert!(accepted_output
            .as_ref()
            .is_some_and(|(_, text)| text.contains("## Workspace source audit")));
        let disk_text = std::fs::read_to_string(workspace_root.join("marketing-brief.md"))
            .expect("read selected artifact");
        assert!(disk_text.contains("## Workspace source audit"));

        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn completed_brief_without_read_is_blocked_even_if_it_looks_confident() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true
                }
            })),
        };
        let tool_telemetry = json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "websearch", "write"],
            "workspace_inspection_used": true,
            "web_research_used": true
        });

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Files reviewed\n- tandem-reference/readmes/repo-README.md\n- tandem-reference/readmes/engine-README.md\n".to_string(),
            )),
            &tool_telemetry,
            None,
        );

        assert_eq!(status, "needs_repair");
        assert_eq!(
            reason.as_deref(),
            Some(
                "research brief cited workspace sources without using read, so source-backed validation is incomplete"
            )
        );
        assert_eq!(approved, Some(true));
    }

    #[test]
    fn brief_with_timed_out_websearch_is_blocked_when_web_research_is_required() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true
                }
            })),
        };
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-websearch-timeout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace_root).expect("create workspace root");
        let snapshot = std::collections::BTreeSet::new();

        let brief_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Files reviewed\n- tandem-reference/readmes/repo-README.md\n\n## Web sources reviewed\n- websearch attempt timed out.\n".to_string();
        std::fs::write(workspace_root.join("marketing-brief.md"), &brief_text)
            .expect("seed artifact");

        let mut session = Session::new(
            Some("session-timeout".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "read".to_string(),
                    args: json!({"path":"tandem-reference/readmes/repo-README.md"}),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "websearch".to_string(),
                    args: json!({"query":"ai coding agents market"}),
                    result: Some(json!({
                        "output": "Search timed out. No results received.",
                        "metadata": { "error": "timeout" }
                    })),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path": "marketing-brief.md",
                        "content": brief_text
                    }),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));

        let tool_telemetry = summarize_automation_tool_activity(
            &node,
            &session,
            &[
                "glob".to_string(),
                "read".to_string(),
                "websearch".to_string(),
                "write".to_string(),
            ],
        );
        assert_eq!(
            tool_telemetry
                .get("web_research_used")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            tool_telemetry
                .get("web_research_succeeded")
                .and_then(Value::as_bool),
            Some(false)
        );

        let (accepted_output, metadata, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root
                .to_str()
                .expect("workspace root string"),
            "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
            &tool_telemetry,
            None,
            Some(("marketing-brief.md".to_string(), brief_text.clone())),
            &snapshot,
        );

        assert!(accepted_output.is_some());
        assert_eq!(
            metadata
                .get("semantic_block_reason")
                .and_then(Value::as_str),
            Some("research completed without required current web research")
        );
        assert_eq!(
            rejected.as_deref(),
            Some("research completed without required current web research")
        );
        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
            accepted_output.as_ref(),
            &tool_telemetry,
            Some(&metadata),
        );
        assert_eq!(status, "needs_repair");
        assert_eq!(
            reason.as_deref(),
            Some("research completed without required current web research")
        );
        assert_eq!(approved, Some(true));

        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn brief_prewrite_requirements_enable_repair_and_coverage_mode() {
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true
                }
            })),
        };
        let requirements = automation_node_prewrite_requirements(
            &node,
            &[
                "glob".to_string(),
                "read".to_string(),
                "websearch".to_string(),
                "write".to_string(),
            ],
        )
        .expect("prewrite requirements");
        assert!(requirements.workspace_inspection_required);
        assert!(requirements.web_research_required);
        assert!(requirements.concrete_read_required);
        assert!(requirements.successful_web_research_required);
        assert!(requirements.repair_on_unmet_requirements);
        assert_eq!(
            requirements.coverage_mode,
            PrewriteCoverageMode::ResearchCorpus
        );
    }

    #[test]
    fn brief_with_unreviewed_discovered_files_is_blocked_with_structured_metadata() {
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-brief-coverage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(workspace_root.join("docs")).expect("create workspace");
        std::fs::write(
            workspace_root.join("docs/one.md"),
            "# One\nsource content\n",
        )
        .expect("write one");
        std::fs::write(
            workspace_root.join("docs/two.md"),
            "# Two\nsource content\n",
        )
        .expect("write two");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root string"),
        );
        let node = AutomationFlowNode {
            node_id: "research".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Research".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": false
                }
            })),
        };
        let brief_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Files reviewed\n- docs/one.md\n".to_string();
        std::fs::write(workspace_root.join("marketing-brief.md"), &brief_text).expect("seed brief");
        let mut session = Session::new(
            Some("coverage mismatch".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "glob".to_string(),
                    args: json!({"pattern":"docs/**/*.md"}),
                    result: Some(json!({"output": format!(
                        "{}\n{}",
                        workspace_root.join("docs/one.md").display(),
                        workspace_root.join("docs/two.md").display()
                    )})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "read".to_string(),
                    args: json!({"path":"docs/one.md"}),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"marketing-brief.md","content":brief_text}),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));
        let tool_telemetry = summarize_automation_tool_activity(
            &node,
            &session,
            &["glob".to_string(), "read".to_string(), "write".to_string()],
        );
        let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root string"),
            "Done\n\n{\"status\":\"completed\"}",
            &tool_telemetry,
            None,
            Some(("marketing-brief.md".to_string(), brief_text)),
            &snapshot,
        );
        assert_eq!(
            rejected.as_deref(),
            Some("research completed without covering or explicitly skipping relevant discovered files")
        );
        assert_eq!(
            metadata
                .get("unreviewed_relevant_paths")
                .and_then(Value::as_array)
                .map(|values| values.len()),
            Some(1)
        );
        assert!(metadata
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|values| values
                .iter()
                .any(|value| value.as_str() == Some("relevant_files_not_reviewed_or_skipped"))));

        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn research_brief_without_source_coverage_flag_gets_semantic_block_reason_and_needs_repair() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-research-no-coverage-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let brief_text =
            "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n"
                .to_string();
        std::fs::write(workspace_root.join("marketing-brief.md"), &brief_text).expect("seed brief");
        let node = AutomationFlowNode {
            node_id: "research-brief".to_string(),
            agent_id: "researcher".to_string(),
            objective: "Write marketing brief".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true
                }
            })),
        };
        let mut session = Session::new(
            Some("research-no-coverage".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "glob".to_string(),
                    args: json!({"pattern":"docs/**/*.md"}),
                    result: Some(json!({"output": ""})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"marketing-brief.md","content":brief_text}),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));
        let requested_tools = vec![
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ];
        let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
        let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root string"),
            "Done\n\n{\"status\":\"completed\"}",
            &tool_telemetry,
            None,
            Some(("marketing-brief.md".to_string(), brief_text.clone())),
            &std::collections::BTreeSet::new(),
        );

        assert_eq!(
            rejected.as_deref(),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(
            artifact_validation
                .get("semantic_block_reason")
                .and_then(Value::as_str),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(
            artifact_validation
                .get("validation_outcome")
                .and_then(Value::as_str),
            Some("needs_repair")
        );

        let (status, reason, approved) = detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written.",
            Some(&("marketing-brief.md".to_string(), brief_text)),
            &tool_telemetry,
            Some(&artifact_validation),
        );

        assert_eq!(status, "needs_repair");
        assert_eq!(
            reason.as_deref(),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(approved, None);

        let _ = std::fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn research_brief_full_pipeline_overrides_llm_blocked_to_needs_repair_without_source_coverage_flag(
    ) {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-research-full-pipeline-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let brief_text =
            "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n"
                .to_string();
        std::fs::write(workspace_root.join("marketing-brief.md"), &brief_text).expect("seed brief");
        let node = AutomationFlowNode {
            node_id: "research-brief".to_string(),
            agent_id: "researcher".to_string(),
            objective: "Write marketing brief".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "brief".to_string(),
                validator: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "output_path": "marketing-brief.md",
                    "web_research_expected": true
                }
            })),
        };
        let mut session = Session::new(
            Some("research-full-pipeline".to_string()),
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root string")
                    .to_string(),
            ),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "glob".to_string(),
                    args: json!({"pattern":"docs/**/*.md"}),
                    result: Some(json!({"output": ""})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"marketing-brief.md","content":brief_text}),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));
        let requested_tools = vec![
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ];
        let session_text =
            "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}";
        let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
        let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root string"),
            session_text,
            &tool_telemetry,
            None,
            Some(("marketing-brief.md".to_string(), brief_text.clone())),
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(
            rejected.as_deref(),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert_eq!(
            artifact_validation
                .get("semantic_block_reason")
                .and_then(Value::as_str),
            Some("research completed without concrete file reads or required source coverage")
        );

        let output = wrap_automation_node_output(
            &node,
            &session,
            &requested_tools,
            "sess-research-full-pipeline",
            session_text,
            accepted_output,
            Some(artifact_validation),
        );

        assert_eq!(
            output.get("status").and_then(Value::as_str),
            Some("needs_repair")
        );
        assert_eq!(
            output.get("blocked_reason").and_then(Value::as_str),
            Some("research completed without concrete file reads or required source coverage")
        );
        assert!(!automation_output_is_blocked(&output));
        assert!(automation_output_needs_repair(&output));
        assert!(!automation_output_repair_exhausted(&output));

        let _ = std::fs::remove_dir_all(workspace_root);
    }
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
