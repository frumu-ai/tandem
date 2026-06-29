use serde::{Deserialize, Serialize};
use tandem_types::TenantContext;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum QueueReason {
    Capacity,
    WorkspaceLock,
    RateLimit,
    RetryBackoff,
}

impl QueueReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::WorkspaceLock => "workspace_lock",
            Self::RateLimit => "rate_limit",
            Self::RetryBackoff => "retry_backoff",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchedulerMetadata {
    #[serde(default = "default_tenant_context")]
    pub tenant_context: TenantContext,
    pub queue_reason: Option<QueueReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limited_provider: Option<String>,
    #[serde(default)]
    pub queued_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_backoff_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_reason: Option<String>,
}

fn default_tenant_context() -> TenantContext {
    TenantContext::local_implicit()
}
