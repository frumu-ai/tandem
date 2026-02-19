#![recursion_limit = "256"]

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_memory::{GovernedMemoryTier, MemoryClassification, MemoryContentKind, MemoryPartition};
use tandem_orchestrator::MissionState;
use tandem_types::EngineEvent;
use tokio::fs;
use tokio::sync::RwLock;

use tandem_channels::config::{ChannelsConfig, DiscordConfig, SlackConfig, TelegramConfig};
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage,
};
use tandem_providers::ProviderRegistry;
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::ToolRegistry;

mod agent_teams;
mod http;
pub mod webui;

pub use agent_teams::AgentTeamRuntime;
pub use http::serve;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfigFile {
    pub bot_token: String,
    #[serde(default = "default_allow_all")]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub mention_only: bool,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EffectiveAppConfig {
    #[serde(default)]
    pub channels: ChannelsConfigFile,
    #[serde(default)]
    pub web_ui: WebUiConfig,
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

#[derive(Debug, Clone, Serialize)]
pub struct RoutineTriggerPlan {
    pub routine_id: String,
    pub run_count: u32,
    pub scheduled_at_ms: u64,
    pub next_fire_at_ms: u64,
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
    pub routines_path: PathBuf,
    pub agent_teams: AgentTeamRuntime,
    pub web_ui_enabled: Arc<AtomicBool>,
    pub web_ui_prefix: Arc<std::sync::RwLock<String>>,
    pub server_base_url: Arc<std::sync::RwLock<String>>,
    pub channels_runtime: Arc<tokio::sync::Mutex<ChannelRuntime>>,
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
            routines_path: resolve_routines_path(),
            agent_teams: AgentTeamRuntime::new(resolve_agent_team_audit_path()),
            web_ui_enabled: Arc::new(AtomicBool::new(false)),
            web_ui_prefix: Arc::new(std::sync::RwLock::new("/admin".to_string())),
            server_base_url: Arc::new(std::sync::RwLock::new("http://127.0.0.1:39731".to_string())),
            channels_runtime: Arc::new(tokio::sync::Mutex::new(ChannelRuntime::default())),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.runtime.get().is_some()
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

    pub async fn set_phase(&self, phase: impl Into<String>) {
        let mut startup = self.startup.write().await;
        startup.phase = phase.into();
    }

    pub async fn mark_ready(&self, runtime: RuntimeState) -> anyhow::Result<()> {
        self.runtime
            .set(runtime)
            .map_err(|_| anyhow::anyhow!("runtime already initialized"))?;
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
        let _ = self.load_shared_resources().await;
        let _ = self.load_routines().await;
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
        let parsed = serde_json::from_str::<std::collections::HashMap<String, RoutineSpec>>(&raw)
            .unwrap_or_default();
        let mut guard = self.routines.write().await;
        *guard = parsed;
        Ok(())
    }

    pub async fn persist_routines(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.routines_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.routines.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.routines_path, payload).await?;
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

        let interval = match routine.schedule {
            RoutineSchedule::IntervalSeconds { seconds } => {
                if seconds == 0 {
                    return Err(RoutineStoreError::InvalidSchedule {
                        detail: "interval_seconds must be > 0".to_string(),
                    });
                }
                Some(seconds)
            }
            RoutineSchedule::Cron { .. } => None,
        };
        if routine.next_fire_at_ms.is_none() {
            routine.next_fire_at_ms = Some(now_ms().saturating_add(interval.unwrap_or(60) * 1000));
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

        if let Err(error) = self.persist_routines().await {
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
            let Some(interval_ms) = routine_interval_ms(&routine.schedule) else {
                continue;
            };
            if now_ms < next_fire_at_ms {
                continue;
            }
            let (run_count, next_fire_at_ms) = compute_misfire_plan(
                now_ms,
                next_fire_at_ms,
                interval_ms,
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
        }),
        server_base_url: state.server_base_url(),
        api_token: state.api_token().await.unwrap_or_default(),
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

fn resolve_run_stale_ms() -> u64 {
    std::env::var("TANDEM_RUN_STALE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(120_000)
        .clamp(30_000, 600_000)
}

fn resolve_shared_resources_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("shared_resources.json");
        }
    }
    PathBuf::from(".tandem").join("shared_resources.json")
}

fn resolve_routines_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("routines.json");
        }
    }
    PathBuf::from(".tandem").join("routines.json")
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
    PathBuf::from(".tandem")
        .join("agent-team")
        .join("audit.log.jsonl")
}

fn routine_interval_ms(schedule: &RoutineSchedule) -> Option<u64> {
    match schedule {
        RoutineSchedule::IntervalSeconds { seconds } => Some(seconds.saturating_mul(1000)),
        RoutineSchedule::Cron { .. } => None,
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

fn extract_event_session_id(properties: &Value) -> Option<String> {
    properties
        .get("sessionID")
        .or_else(|| properties.get("sessionId"))
        .or_else(|| properties.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_event_run_id(properties: &Value) -> Option<String> {
    properties
        .get("runID")
        .or_else(|| properties.get("run_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
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
            let (phase, tool_active) = match part_type {
                "tool-invocation" => ("tool", true),
                "tool-result" => ("run", false),
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

pub async fn run_status_indexer(state: AppState) {
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
                            "runCount": plan.run_count,
                            "scheduledAtMs": plan.scheduled_at_ms,
                            "nextFireAtMs": plan.next_fire_at_ms,
                        }),
                    ));
                }
                RoutineExecutionDecision::RequiresApproval { reason } => {
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
                            "runCount": plan.run_count,
                            "triggerType": "scheduled",
                            "reason": reason,
                        }),
                    ));
                }
                RoutineExecutionDecision::Blocked { reason } => {
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
                            "runCount": plan.run_count,
                            "triggerType": "scheduled",
                            "reason": reason,
                        }),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state_with_path(path: PathBuf) -> AppState {
        let mut state = AppState::new_starting("test-attempt".to_string(), true);
        state.shared_resources_path = path;
        state.routines_path = tmp_routines_file("shared-state");
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
}
