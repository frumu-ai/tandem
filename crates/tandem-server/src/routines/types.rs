use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::{EngineEvent, TenantContext};

pub use tandem_automation::RoutineMisfirePolicy;

const TENANT_SCOPED_ROUTINE_PREFIX: &str = "__tenant_routine__::";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RoutineIdentity {
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    pub routine_id: String,
}

impl RoutineIdentity {
    pub fn new(routine_id: impl Into<String>, tenant_context: &TenantContext) -> Self {
        Self {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
            routine_id: routine_id.into(),
        }
    }

    pub fn matches_tenant(&self, tenant_context: &TenantContext) -> bool {
        self.org_id == tenant_context.org_id
            && self.workspace_id == tenant_context.workspace_id
            && self.deployment_id == tenant_context.deployment_id
    }

    pub(crate) fn storage_key(&self) -> String {
        if self.org_id == "local" && self.workspace_id == "local" && self.deployment_id.is_none() {
            return self.routine_id.clone();
        }
        let component = |value: &str| {
            value
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        format!(
            "{TENANT_SCOPED_ROUTINE_PREFIX}{}::{}::{}::{}",
            component(&self.org_id),
            component(&self.workspace_id),
            component(self.deployment_id.as_deref().unwrap_or("")),
            self.routine_id,
        )
    }
}

pub(crate) fn tenant_scoped_engine_event(
    event_type: impl Into<String>,
    tenant_context: &TenantContext,
    mut properties: Value,
) -> EngineEvent {
    if let Some(properties) = properties.as_object_mut() {
        properties.insert(
            "tenantContext".to_string(),
            serde_json::to_value(tenant_context).unwrap_or(Value::Null),
        );
    }
    EngineEvent::new(event_type, properties)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineSchedule {
    IntervalSeconds { seconds: u64 },
    Cron { expression: String },
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
    #[serde(default, skip_serializing_if = "TenantContext::is_local_implicit")]
    pub tenant_context: TenantContext,
    pub name: String,
    pub status: RoutineStatus,
    pub schedule: RoutineSchedule,
    pub timezone: String,
    pub misfire_policy: RoutineMisfirePolicy,
    pub entrypoint: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub output_targets: Vec<String>,
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
    #[serde(default, skip_serializing_if = "TenantContext::is_local_implicit")]
    pub tenant_context: TenantContext,
    pub trigger_type: String,
    pub run_count: u32,
    pub fired_at_ms: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineRunStatus {
    Queued,
    PendingApproval,
    Running,
    Paused,
    BlockedPolicy,
    Denied,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRunArtifact {
    pub artifact_id: String,
    pub uri: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExternalActionRecord {
    pub action_id: String,
    pub operation: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routine_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRunRecord {
    pub run_id: String,
    pub routine_id: String,
    #[serde(default, skip_serializing_if = "TenantContext::is_local_implicit")]
    pub tenant_context: TenantContext,
    pub trigger_type: String,
    pub run_count: u32,
    pub status: RoutineRunStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fired_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    pub requires_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub entrypoint: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub output_targets: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<RoutineRunArtifact>,
    #[serde(default)]
    pub active_session_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_session_id: Option<String>,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone)]
pub struct RoutineSessionPolicy {
    pub session_id: String,
    pub run_id: String,
    pub routine_id: String,
    pub tenant_context: TenantContext,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoutineTriggerPlan {
    pub identity: RoutineIdentity,
    pub tenant_context: TenantContext,
    pub run_count: u32,
    pub scheduled_at_ms: u64,
    pub next_fire_at_ms: u64,
}
