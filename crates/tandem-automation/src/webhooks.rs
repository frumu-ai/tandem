use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::{
    DataClass, PrincipalRef, ResourceScope, SecretRef, TenantContext, ToolRiskTier,
};

fn default_tenant_context() -> TenantContext {
    TenantContext::local_implicit()
}

fn default_data_class() -> DataClass {
    DataClass::Internal
}

fn default_signature_scheme() -> AutomationWebhookSignatureScheme {
    AutomationWebhookSignatureScheme::HmacSha256V1
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookSignatureScheme {
    #[default]
    HmacSha256V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookDeliveryStatus {
    Received,
    Accepted,
    Rejected,
    Duplicate,
    Disabled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationWebhookSecretMetadata {
    pub secret_ref: SecretRef,
    pub secret_digest: String,
    #[serde(default)]
    pub secret_version: u64,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotated_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotated_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationWebhookTriggerRecord {
    pub trigger_id: String,
    pub automation_id: String,
    #[serde(default = "default_tenant_context")]
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_principal: Option<PrincipalRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owning_org_unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_scope: Option<ResourceScope>,
    #[serde(default = "default_data_class")]
    pub default_data_class: DataClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_risk_tier: Option<ToolRiskTier>,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_kind: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub public_path_token: String,
    #[serde(default = "default_signature_scheme")]
    pub signature_scheme: AutomationWebhookSignatureScheme,
    pub secret: AutomationWebhookSecretMetadata,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_received_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accepted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_rejected_at_ms: Option<u64>,
}

impl AutomationWebhookTriggerRecord {
    pub fn tenant_matches(&self, tenant_context: &TenantContext) -> bool {
        self.tenant_context.org_id == tenant_context.org_id
            && self.tenant_context.workspace_id == tenant_context.workspace_id
            && self.tenant_context.deployment_id == tenant_context.deployment_id
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationWebhookDeliveryRecord {
    pub delivery_id: String,
    pub trigger_id: String,
    pub automation_id: String,
    #[serde(default = "default_tenant_context")]
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_id: Option<String>,
    pub body_digest: String,
    pub status: AutomationWebhookDeliveryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_run_id: Option<String>,
    pub received_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_at_ms: Option<u64>,
    #[serde(default)]
    pub sanitized_preview: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_event_id: Option<String>,
}

impl AutomationWebhookDeliveryRecord {
    pub fn tenant_matches(&self, tenant_context: &TenantContext) -> bool {
        self.tenant_context.org_id == tenant_context.org_id
            && self.tenant_context.workspace_id == tenant_context.workspace_id
            && self.tenant_context.deployment_id == tenant_context.deployment_id
    }
}
