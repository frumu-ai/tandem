use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::{
    DataClass, PrincipalRef, ResourceScope, SecretRef, TenantContext, ToolRiskTier,
};

use crate::enterprise_scope::AutomationEnterpriseScope;

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

const GENERIC_PROVIDER_EVENT_ID_HEADERS: &[&str] = &[
    "x-tandem-webhook-event-id",
    "x-webhook-event-id",
    "x-event-id",
];
const GITHUB_PROVIDER_EVENT_ID_HEADERS: &[&str] = &[
    "x-github-delivery",
    "x-tandem-webhook-event-id",
    "x-webhook-event-id",
    "x-event-id",
];
const GITLAB_PROVIDER_EVENT_ID_HEADERS: &[&str] = &[
    "x-gitlab-event-uuid",
    "x-gitlab-delivery",
    "x-tandem-webhook-event-id",
    "x-webhook-event-id",
    "x-event-id",
];
const LINEAR_PROVIDER_EVENT_ID_HEADERS: &[&str] = &[
    "linear-delivery",
    "x-linear-delivery",
    "x-tandem-webhook-event-id",
    "x-webhook-event-id",
    "x-event-id",
];
const STRIPE_PROVIDER_EVENT_ID_HEADERS: &[&str] = &[
    "x-stripe-event-id",
    "stripe-event-id",
    "x-tandem-webhook-event-id",
    "x-webhook-event-id",
    "x-event-id",
];

pub fn normalize_automation_webhook_provider(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_ascii_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if normalized.is_empty() {
        return None;
    }
    let canonical = match normalized.as_str() {
        "custom" | "http" => "generic",
        "gh" | "github.com" => "github",
        "gitlab.com" => "gitlab",
        "linear.app" => "linear",
        "slack.com" => "slack",
        "stripe.com" => "stripe",
        _ => normalized.as_str(),
    };
    Some(canonical.to_string())
}

pub fn normalize_automation_webhook_provider_event_kind(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

pub fn automation_webhook_provider_event_id_headers(provider: &str) -> &'static [&'static str] {
    match normalize_automation_webhook_provider(provider).as_deref() {
        Some("github") => GITHUB_PROVIDER_EVENT_ID_HEADERS,
        Some("gitlab") => GITLAB_PROVIDER_EVENT_ID_HEADERS,
        Some("linear") => LINEAR_PROVIDER_EVENT_ID_HEADERS,
        Some("stripe") => STRIPE_PROVIDER_EVENT_ID_HEADERS,
        _ => GENERIC_PROVIDER_EVENT_ID_HEADERS,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookSignatureScheme {
    #[default]
    HmacSha256V1,
    GithubHmacSha256,
    SharedSecretHeaderV1,
    UnsignedDevMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookDeliveryStatus {
    Received,
    Accepted,
    Rejected,
    Duplicate,
    Suppressed,
    Disabled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookDedupeResult {
    Accepted,
    Duplicate,
    Replay,
    Conflict,
    IgnoredFeedbackLoop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookFeedbackLoopOutcome {
    Suppressed,
    Allowed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationWebhookFeedbackLoopDecision {
    pub outcome: AutomationWebhookFeedbackLoopOutcome,
    pub reason_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_action_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_target: Option<String>,
    #[serde(default)]
    pub allow_self_feedback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookCorrelationOutcome {
    Received,
    NewRun,
    WakeRun,
    Duplicate,
    Suppressed,
    Rejected,
    DeadLetter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationWebhookCorrelationRecord {
    pub outcome: AutomationWebhookCorrelationOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woken_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woken_wait_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of_delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
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
    #[serde(default)]
    pub name: String,
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

    pub fn enterprise_scope(&self) -> Option<AutomationEnterpriseScope> {
        let scope = AutomationEnterpriseScope {
            owner_principal: self.owner_principal.clone(),
            owning_org_unit_id: self.owning_org_unit_id.clone(),
            resource_scope: self.resource_scope.clone(),
            data_classes: vec![self.default_data_class],
            risk_tier: self.default_risk_tier,
            policy_version_id: None,
            delegation_grant_ids: Vec::new(),
        }
        .normalized();
        (!scope.is_empty()).then_some(scope)
    }
}

fn default_raw_payload_retention_ms() -> u64 {
    30 * 24 * 60 * 60 * 1000
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationWebhookEventRetentionPolicy {
    #[serde(default = "default_raw_payload_retention_ms")]
    pub raw_payload_retention_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_after_ms: Option<u64>,
    #[serde(default)]
    pub headers_redacted: bool,
}

impl Default for AutomationWebhookEventRetentionPolicy {
    fn default() -> Self {
        Self {
            raw_payload_retention_ms: default_raw_payload_retention_ms(),
            delete_after_ms: None,
            headers_redacted: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationWebhookRawEventRecord {
    pub event_id: String,
    pub trigger_id: String,
    pub automation_id: String,
    #[serde(default = "default_tenant_context")]
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_scope: Option<AutomationEnterpriseScope>,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_id: Option<String>,
    pub body_digest: String,
    pub headers_digest: String,
    #[serde(default)]
    pub headers_redacted: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    pub payload_ref: String,
    pub payload_bytes: u64,
    pub status: AutomationWebhookDeliveryStatus,
    pub received_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_result: Option<AutomationWebhookDedupeResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of_delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woken_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woken_wait_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback_loop: Option<AutomationWebhookFeedbackLoopDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<AutomationWebhookCorrelationRecord>,
    #[serde(default)]
    pub retention_policy: AutomationWebhookEventRetentionPolicy,
}

impl AutomationWebhookRawEventRecord {
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
    pub enterprise_scope: Option<AutomationEnterpriseScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_id: Option<String>,
    pub body_digest: String,
    pub status: AutomationWebhookDeliveryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_result: Option<AutomationWebhookDedupeResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of_delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_scheme: Option<AutomationWebhookSignatureScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woken_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woken_wait_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback_loop: Option<AutomationWebhookFeedbackLoopDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<AutomationWebhookCorrelationRecord>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_normalization_canonicalizes_known_aliases() {
        assert_eq!(
            normalize_automation_webhook_provider(" GitHub.com "),
            Some("github".to_string())
        );
        assert_eq!(
            normalize_automation_webhook_provider("custom"),
            Some("generic".to_string())
        );
        assert_eq!(normalize_automation_webhook_provider("  "), None);
    }

    #[test]
    fn provider_event_id_headers_prefer_provider_specific_headers() {
        assert_eq!(
            automation_webhook_provider_event_id_headers("github")[0],
            "x-github-delivery"
        );
        assert_eq!(
            automation_webhook_provider_event_id_headers("unknown-provider")[0],
            "x-tandem-webhook-event-id"
        );
    }
}
