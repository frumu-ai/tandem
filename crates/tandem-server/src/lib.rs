use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::RwLock;

use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage,
};
use tandem_providers::ProviderRegistry;
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::ToolRegistry;

mod http;

pub use http::serve;

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
        return std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().to_string());
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
    pub engine_leases: Arc<RwLock<std::collections::HashMap<String, EngineLease>>>,
    pub run_registry: RunRegistry,
    pub run_stale_ms: u64,
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
            engine_leases: Arc::new(RwLock::new(std::collections::HashMap::new())),
            run_registry: RunRegistry::new(),
            run_stale_ms: resolve_run_stale_ms(),
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
}

fn resolve_run_stale_ms() -> u64 {
    std::env::var("TANDEM_RUN_STALE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(120_000)
        .clamp(30_000, 600_000)
}

impl Deref for AppState {
    type Target = RuntimeState;

    fn deref(&self) -> &Self::Target {
        self.runtime
            .get()
            .expect("runtime accessed before startup completion")
    }
}
