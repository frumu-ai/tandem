//! OAuth callback session state owned as a single AppState manager.
//!
//! Provider OAuth and MCP OAuth both keep short-lived callback sessions while a
//! browser authorization flow is pending. Keeping those maps behind this manager
//! gives OAuth a clear AppState ownership boundary. If code ever needs both
//! locks, take provider sessions before MCP sessions.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

/// Pending OAuth callback sessions for runtime-managed provider and MCP flows.
#[derive(Clone)]
pub struct OAuthState {
    provider_sessions:
        Arc<RwLock<HashMap<String, crate::http::config_providers::ProviderOAuthSessionRecord>>>,
    mcp_sessions: Arc<RwLock<HashMap<String, crate::http::mcp::McpOAuthSessionRecord>>>,
}

impl OAuthState {
    pub fn new() -> Self {
        Self {
            provider_sessions: Arc::new(RwLock::new(HashMap::new())),
            mcp_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) fn provider_sessions(
        &self,
    ) -> &Arc<RwLock<HashMap<String, crate::http::config_providers::ProviderOAuthSessionRecord>>>
    {
        &self.provider_sessions
    }

    pub(crate) fn mcp_sessions(
        &self,
    ) -> &Arc<RwLock<HashMap<String, crate::http::mcp::McpOAuthSessionRecord>>> {
        &self.mcp_sessions
    }
}

impl Default for OAuthState {
    fn default() -> Self {
        Self::new()
    }
}
