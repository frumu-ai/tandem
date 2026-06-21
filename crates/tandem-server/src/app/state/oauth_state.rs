//! OAuth callback session state owned as a single AppState manager.
//!
//! Provider OAuth and MCP OAuth both keep short-lived callback sessions while a
//! browser authorization flow is pending. Keeping those maps behind this manager
//! gives OAuth a clear AppState ownership boundary. If code ever needs both
//! locks, take provider sessions before MCP sessions.

use std::collections::HashMap;
use std::sync::Arc;

use crate::http::{config_providers::ProviderOAuthSessionRecord, mcp::McpOAuthSessionRecord};
use tokio::sync::RwLock;

type ProviderOAuthSessions = HashMap<String, ProviderOAuthSessionRecord>;
type McpOAuthSessions = HashMap<String, McpOAuthSessionRecord>;

/// Pending OAuth callback sessions for runtime-managed provider and MCP flows.
#[derive(Clone)]
pub struct OAuthState {
    provider_sessions: Arc<RwLock<ProviderOAuthSessions>>,
    mcp_sessions: Arc<RwLock<McpOAuthSessions>>,
}

impl OAuthState {
    pub fn new() -> Self {
        Self {
            provider_sessions: Arc::new(RwLock::new(HashMap::new())),
            mcp_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
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
