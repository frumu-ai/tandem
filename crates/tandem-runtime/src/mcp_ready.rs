//! MCP readiness gate (Invariant 2 of `docs/SPINE.md`).
//!
//! Every concrete MCP tool call must reach a connected server through a
//! single readiness check, or fail fast with a typed error. Before this
//! module landed, six production sites each reinvented `if !connected
//! { connect() }` with their own retry policy and stringly-typed error
//! handling (commits `852c453`, `f6bf753`, plus inline copies in the
//! bug monitor and automation paths). One missed site is a stuck or
//! panicking run.
//!
//! [`McpRegistry::ensure_ready`] is the single gate. Callers pass an
//! [`EnsureReadyPolicy`] (default = single attempt, no backoff) and
//! receive a typed [`McpReadyError`] when readiness can't be achieved.
//!
//! Phase 2 migrates the bedrock site (`McpRegistry::call_tool`) and the
//! most recently reinvented retry helper
//! (`automation_connect_mcp_server_with_retry`). The remaining external
//! callers — `bug_monitor_github`, `automation/capability_impl`,
//! `automation/logic_parts/part01.rs:880`,
//! `automation/logic/part01_parts/part01.rs:608`,
//! `app_state_impl_parts/part02.rs:378` — migrate in the follow-up.
//!
//! Once those land, `McpRegistry::connect` can drop to `pub(crate)` and
//! the gate becomes compile-time enforced. Until then it is convention
//! plus this module's doc comment.

use std::time::Duration;

use crate::mcp::{McpRegistry, McpServer};

/// Why an MCP server is not ready for tool dispatch.
///
/// Replaces the ad-hoc `bool` and `String` returns previously used at
/// each retry site. Callers `match` exhaustively, so a future variant
/// (e.g. `AuthRequired`) cannot be silently ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpReadyError {
    /// No server is registered under this name.
    NotFound,
    /// The server is registered but disabled.
    Disabled,
    /// All connection attempts in the configured retry window failed.
    /// `last_error` is the server's last reported error, when present.
    PermanentlyFailed { last_error: Option<String> },
}

impl std::fmt::Display for McpReadyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "MCP server not found"),
            Self::Disabled => write!(f, "MCP server is disabled"),
            Self::PermanentlyFailed { last_error } => match last_error {
                Some(detail) => write!(f, "MCP server connect failed: {detail}"),
                None => write!(f, "MCP server connect failed"),
            },
        }
    }
}

impl std::error::Error for McpReadyError {}

/// Retry policy for [`McpRegistry::ensure_ready`].
///
/// Default is a single attempt with no delay — the same shape as the
/// pre-existing inline check at `call_tool`. Use [`Self::with_retries`]
/// when reconnect after transient drop is expected (automation
/// preflight, bug monitor heartbeat).
#[derive(Debug, Clone, Copy)]
pub struct EnsureReadyPolicy {
    pub attempts: usize,
    pub initial_delay_ms: u64,
    pub backoff_factor: u32,
}

impl Default for EnsureReadyPolicy {
    fn default() -> Self {
        Self {
            attempts: 1,
            initial_delay_ms: 0,
            backoff_factor: 1,
        }
    }
}

impl EnsureReadyPolicy {
    /// Retry up to `attempts` times with `initial_delay_ms` before the
    /// second attempt and ×2 backoff thereafter. Matches the timing of
    /// the previous `automation_connect_mcp_server_with_retry` helper
    /// (`attempts=3, initial_delay_ms=750` → delays 0/750/1500ms).
    pub fn with_retries(attempts: usize, initial_delay_ms: u64) -> Self {
        Self {
            attempts: attempts.max(1),
            initial_delay_ms,
            backoff_factor: 2,
        }
    }
}

impl McpRegistry {
    /// Single readiness gate for MCP server access. Returns the current
    /// `McpServer` snapshot when the server is enabled and connected,
    /// or reconnects up to `policy.attempts` times before giving up
    /// with a typed [`McpReadyError`].
    ///
    /// Every "I'm about to use this server" call site should go through
    /// this method. User-initiated connect endpoints (HTTP handlers
    /// where the user explicitly clicked "Connect") still call
    /// [`McpRegistry::connect`] directly — they want a single attempt
    /// with a `bool` answer, not retry-and-throw.
    pub async fn ensure_ready(
        &self,
        server_name: &str,
        policy: EnsureReadyPolicy,
    ) -> Result<McpServer, McpReadyError> {
        let initial = self.list().await.get(server_name).cloned();
        let Some(server) = initial else {
            return Err(McpReadyError::NotFound);
        };
        if !server.enabled {
            return Err(McpReadyError::Disabled);
        }
        if server.connected {
            return Ok(server);
        }

        let attempts = policy.attempts.max(1);
        let mut next_delay_ms = policy.initial_delay_ms;
        for attempt in 0..attempts {
            if attempt > 0 {
                if next_delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(next_delay_ms)).await;
                }
                next_delay_ms =
                    next_delay_ms.saturating_mul(policy.backoff_factor.max(1) as u64);
            }
            if self.connect(server_name).await {
                if let Some(server) = self.list().await.get(server_name).cloned() {
                    if server.connected {
                        return Ok(server);
                    }
                }
            }
        }

        let last_error = self
            .list()
            .await
            .get(server_name)
            .and_then(|s| s.last_error.clone())
            .filter(|e| !e.trim().is_empty());
        Err(McpReadyError::PermanentlyFailed { last_error })
    }
}
