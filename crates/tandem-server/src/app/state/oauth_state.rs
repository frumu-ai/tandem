//! OAuth callback session state owned as a single AppState manager.
//!
//! Provider OAuth and MCP OAuth both keep short-lived callback sessions while a
//! browser authorization flow is pending. Keeping those maps behind this manager
//! gives OAuth a clear AppState ownership boundary. If code ever needs both
//! locks, take provider sessions before MCP sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, Weak};

use crate::http::{config_providers::ProviderOAuthSessionRecord, mcp::McpOAuthSessionRecord};
use tandem_types::TenantContext;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type ProviderOAuthSessions = HashMap<String, ProviderOAuthSessionRecord>;
type McpOAuthSessions = HashMap<String, McpOAuthSessionRecord>;

#[derive(Default)]
struct ProviderCredentialMutationState {
    generation: u64,
}

pub(crate) struct ProviderCredentialMutationGuard(OwnedMutexGuard<ProviderCredentialMutationState>);

impl ProviderCredentialMutationGuard {
    pub(crate) fn generation(&self) -> u64 {
        self.0.generation
    }

    pub(crate) fn advance_generation(&mut self) -> u64 {
        self.0.generation = self.0.generation.wrapping_add(1);
        self.0.generation
    }
}

struct ProviderRefreshTask {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

/// Pending OAuth callback sessions for runtime-managed provider and MCP flows.
#[derive(Clone)]
pub struct OAuthState {
    provider_sessions: Arc<RwLock<ProviderOAuthSessions>>,
    mcp_sessions: Arc<RwLock<McpOAuthSessions>>,
    provider_credential_locks:
        Arc<Mutex<HashMap<String, Weak<Mutex<ProviderCredentialMutationState>>>>>,
    provider_credential_persistence_lock: Arc<Mutex<()>>,
    provider_refresh_task: Arc<StdMutex<Option<ProviderRefreshTask>>>,
}

impl OAuthState {
    pub fn new() -> Self {
        Self {
            provider_sessions: Arc::new(RwLock::new(HashMap::new())),
            mcp_sessions: Arc::new(RwLock::new(HashMap::new())),
            provider_credential_locks: Arc::new(Mutex::new(HashMap::new())),
            provider_credential_persistence_lock: Arc::new(Mutex::new(())),
            provider_refresh_task: Arc::new(StdMutex::new(None)),
        }
    }

    pub(crate) fn spawn_provider_refresh_task<Spawn>(&self, spawn: Spawn) -> bool
    where
        Spawn: FnOnce(CancellationToken) -> JoinHandle<()>,
    {
        let mut task = self
            .provider_refresh_task
            .lock()
            .expect("provider OAuth refresh task lock poisoned");
        if task
            .as_ref()
            .is_some_and(|active| !active.handle.is_finished())
        {
            return false;
        }
        if let Some(finished) = task.take() {
            finished.cancel.cancel();
        }
        let cancel = CancellationToken::new();
        let handle = spawn(cancel.clone());
        *task = Some(ProviderRefreshTask { cancel, handle });
        true
    }

    pub(crate) async fn stop_provider_refresh_task(&self) {
        let task = self
            .provider_refresh_task
            .lock()
            .expect("provider OAuth refresh task lock poisoned")
            .take();
        let Some(mut task) = task else {
            return;
        };
        task.cancel.cancel();
        if tokio::time::timeout(std::time::Duration::from_secs(5), &mut task.handle)
            .await
            .is_err()
        {
            task.handle.abort();
            let _ = task.handle.await;
        }
    }

    #[cfg(test)]
    pub(crate) fn provider_refresh_task_is_running(&self) -> bool {
        self.provider_refresh_task
            .lock()
            .expect("provider OAuth refresh task lock poisoned")
            .as_ref()
            .is_some_and(|task| !task.handle.is_finished())
    }

    pub(crate) async fn provider_credential_guard(
        &self,
        tenant_context: &TenantContext,
        provider_id: &str,
    ) -> ProviderCredentialMutationGuard {
        let key = serde_json::to_string(&(
            tenant_context.org_id.as_str(),
            tenant_context.workspace_id.as_str(),
            tenant_context.deployment_id.as_deref(),
            provider_id.trim().to_ascii_lowercase(),
        ))
        .expect("provider refresh lock key is serializable");
        let lock = {
            let mut locks = self.provider_credential_locks.lock().await;
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(Mutex::new(ProviderCredentialMutationState::default()));
                locks.insert(key, Arc::downgrade(&lock));
                lock
            }
        };
        ProviderCredentialMutationGuard(lock.lock_owned().await)
    }

    pub(crate) async fn provider_credential_persistence_guard(&self) -> OwnedMutexGuard<()> {
        self.provider_credential_persistence_lock
            .clone()
            .lock_owned()
            .await
    }

    pub(crate) fn provider_sessions(&self) -> &Arc<RwLock<ProviderOAuthSessions>> {
        &self.provider_sessions
    }

    pub(crate) fn mcp_sessions(&self) -> &Arc<RwLock<McpOAuthSessions>> {
        &self.mcp_sessions
    }

    pub(crate) async fn insert_mcp_session(
        &self,
        session_id: String,
        session: McpOAuthSessionRecord,
    ) {
        self.mcp_sessions.write().await.insert(session_id, session);
    }

    pub(crate) async fn find_mcp_session<F>(&self, mut matches: F) -> Option<McpOAuthSessionRecord>
    where
        F: FnMut(&McpOAuthSessionRecord) -> bool,
    {
        self.mcp_sessions
            .read()
            .await
            .values()
            .find(|session| matches(session))
            .cloned()
    }

    pub(crate) async fn find_mcp_session_id<F>(&self, mut matches: F) -> Option<String>
    where
        F: FnMut(&McpOAuthSessionRecord) -> bool,
    {
        self.mcp_sessions
            .read()
            .await
            .iter()
            .find_map(|(session_id, session)| matches(session).then(|| session_id.clone()))
    }

    pub(crate) async fn get_mcp_session(&self, session_id: &str) -> Option<McpOAuthSessionRecord> {
        self.mcp_sessions.read().await.get(session_id).cloned()
    }

    pub(crate) async fn retain_mcp_sessions<F>(&self, mut keep: F) -> usize
    where
        F: FnMut(&McpOAuthSessionRecord) -> bool,
    {
        let mut sessions = self.mcp_sessions.write().await;
        let before = sessions.len();
        sessions.retain(|_, session| keep(session));
        before.saturating_sub(sessions.len())
    }

    pub(crate) async fn update_mcp_session<F>(&self, session_id: &str, update: F) -> bool
    where
        F: FnOnce(&mut McpOAuthSessionRecord),
    {
        let mut sessions = self.mcp_sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            update(session);
            true
        } else {
            false
        }
    }
}

impl Default for OAuthState {
    fn default() -> Self {
        Self::new()
    }
}
