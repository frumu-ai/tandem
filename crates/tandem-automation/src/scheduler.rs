use serde::{Deserialize, Serialize};
use tandem_types::TenantContext;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum QueueReason {
    Capacity,
    WorkspaceLock,
    RateLimit,
}

impl QueueReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::WorkspaceLock => "workspace_lock",
            Self::RateLimit => "rate_limit",
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
}

fn default_tenant_context() -> TenantContext {
    TenantContext::local_implicit()
}
