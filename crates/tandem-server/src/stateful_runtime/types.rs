use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::{DataClass, PrincipalRef, ResourceScope, TenantContext, ToolRiskTier};

use super::phases::{StatefulWorkflowPhase, StatefulWorkflowPhaseTransitionRecord};

pub const STATEFUL_RUNTIME_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulRuntimeScope {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owning_org_unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_principal: Option<PrincipalRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_scope: Option<ResourceScope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_tier: Option<ToolRiskTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegation_grant_ids: Vec<String>,
}

impl Default for StatefulRuntimeScope {
    fn default() -> Self {
        Self::local_implicit()
    }
}

impl StatefulRuntimeScope {
    pub fn from_tenant_context(tenant_context: TenantContext) -> Self {
        Self {
            schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
            tenant_context,
            owning_org_unit_id: None,
            owner_principal: None,
            resource_scope: None,
            data_classes: Vec::new(),
            risk_tier: None,
            policy_version_id: None,
            delegation_grant_ids: Vec::new(),
        }
    }

    pub fn local_implicit() -> Self {
        Self::from_tenant_context(TenantContext::local_implicit())
    }

    pub fn organization_id(&self) -> &str {
        &self.tenant_context.org_id
    }

    pub fn workspace_id(&self) -> &str {
        &self.tenant_context.workspace_id
    }

    pub fn deployment_id(&self) -> Option<&str> {
        self.tenant_context.deployment_id.as_deref()
    }

    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        if tenant.is_local_implicit() {
            return true;
        }
        self.tenant_context.org_id == tenant.org_id
            && self.tenant_context.workspace_id == tenant.workspace_id
            && self.tenant_context.deployment_id == tenant.deployment_id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulWorkflowRunKind {
    AutomationV2,
    Workflow,
    ContextRun,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulWorkflowRunStatus {
    Queued,
    Running,
    Sleeping,
    AwaitingWebhook,
    AwaitingApproval,
    Pausing,
    Paused,
    Retrying,
    Blocked,
    Completed,
    Failed,
    Cancelled,
    DeadLettered,
    DryRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulWaitKind {
    Timer,
    Webhook,
    Approval,
    ExternalCondition,
    RetryBackoff,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulWaitStatus {
    Waiting,
    Claimed,
    Woken,
    TimedOut,
    Escalated,
    Cancelled,
}

impl StatefulWaitStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Woken | Self::TimedOut | Self::Escalated | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulWaitTimeoutAction {
    Cancel,
    Escalate,
    Remind,
    Resume,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulWaitTimeoutPolicy {
    pub timeout_at_ms: u64,
    pub on_timeout: StatefulWaitTimeoutAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_every_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatefulWebhookWaitMatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

impl StatefulWebhookWaitMatch {
    pub fn has_constraint(&self) -> bool {
        self.trigger_id.is_some()
            || self.provider.is_some()
            || self.provider_event_kind.is_some()
            || self.provider_event_id.is_some()
            || self.body_digest.is_some()
            || self.idempotency_key.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatefulWebhookWaitEvent {
    pub trigger_id: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_id: Option<String>,
    pub body_digest: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulWaitRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub wait_id: String,
    pub run_id: String,
    pub wait_kind: StatefulWaitKind,
    pub status: StatefulWaitStatus,
    pub scope: StatefulRuntimeScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_policy: Option<StatefulWaitTimeoutPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl StatefulWaitRecord {
    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        self.scope.visible_to_tenant(tenant)
    }

    pub fn is_due_at(&self, now_ms: u64) -> bool {
        self.status == StatefulWaitStatus::Waiting
            && self
                .wake_at_ms
                .map(|wake_at_ms| wake_at_ms <= now_ms)
                .unwrap_or(false)
    }

    pub fn claim_is_active_at(&self, now_ms: u64) -> bool {
        self.status == StatefulWaitStatus::Claimed
            && self
                .claim_expires_at_ms
                .map(|expires_at_ms| expires_at_ms > now_ms)
                .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulWorkflowRunRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub run_id: String,
    pub kind: StatefulWorkflowRunKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_run_id: Option<String>,
    pub scope: StatefulRuntimeScope,
    pub status: StatefulWorkflowRunStatus,
    #[serde(default)]
    pub phase: StatefulWorkflowPhase,
    #[serde(default)]
    pub phase_history: Vec<StatefulWorkflowPhaseTransitionRecord>,
    #[serde(default)]
    pub allowed_next_phases: Vec<StatefulWorkflowPhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_wait_kind: Option<StatefulWaitKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_wait_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_definition_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_definition_snapshot_hash: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_context_run_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulRunEventRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub event_id: String,
    pub run_id: String,
    pub seq: u64,
    pub event_type: String,
    pub occurred_at_ms: u64,
    pub scope: StatefulRuntimeScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<PrincipalRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_transition: Option<StatefulWorkflowPhaseTransitionRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_kind: Option<StatefulWaitKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

impl StatefulRunEventRecord {
    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        self.scope.visible_to_tenant(tenant)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulRunSnapshotRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub snapshot_id: String,
    pub run_id: String,
    pub seq: u64,
    pub created_at_ms: u64,
    pub scope: StatefulRuntimeScope,
    pub status: StatefulWorkflowRunStatus,
    #[serde(default)]
    pub phase: StatefulWorkflowPhase,
    #[serde(default)]
    pub phase_history: Vec<StatefulWorkflowPhaseTransitionRecord>,
    #[serde(default)]
    pub allowed_next_phases: Vec<StatefulWorkflowPhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_record_kind: Option<StatefulWorkflowRunKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_definition_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_definition_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl StatefulRunSnapshotRecord {
    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        self.scope.visible_to_tenant(tenant)
    }
}

pub fn default_schema_version() -> u32 {
    STATEFUL_RUNTIME_SCHEMA_VERSION
}
