#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DataBoundaryMode {
    #[default]
    Off,
    Audit,
    Enforce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBoundaryClass {
    Local,
    CustomerHosted,
    ApprovedExternal,
    UnapprovedExternal,
    Prohibited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveDataClass {
    Pii,
    Phi,
    Financial,
    Credential,
    Secret,
    SourceCode,
    CustomerData,
    EmployeeData,
    Legal,
    ProprietaryBusinessData,
    UnknownSensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBoundaryAction {
    Allow,
    AllowWithAudit,
    Redact,
    Tokenize,
    RouteToLocal,
    RequireApproval,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBoundaryFindingSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBoundaryOperationKind {
    ProviderRequest,
    ToolCall,
    MemoryWrite,
    ContextAssembly,
    RuntimeEvent,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBoundaryEventKind {
    Evaluated,
    Redacted,
    Tokenized,
    Blocked,
    ApprovalRequired,
    RoutedLocal,
}

impl DataBoundaryEventKind {
    pub fn event_name(self) -> &'static str {
        match self {
            Self::Evaluated => "data_boundary.evaluated",
            Self::Redacted => "data_boundary.redacted",
            Self::Tokenized => "data_boundary.tokenized",
            Self::Blocked => "data_boundary.blocked",
            Self::ApprovalRequired => "data_boundary.approval_required",
            Self::RoutedLocal => "data_boundary.routed_local",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBoundaryEvidenceKind {
    PayloadHash,
    PolicyRef,
    DetectorRef,
    RuntimeRef,
    SourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DataBoundaryTenantRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryProviderRef {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub boundary_class: ProviderBoundaryClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryOperationRef {
    pub operation_id: String,
    pub kind: DataBoundaryOperationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryEvidenceRef {
    pub kind: DataBoundaryEvidenceKind,
    pub ref_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryPolicy {
    pub policy_id: String,
    pub mode: DataBoundaryMode,
    pub policy_fingerprint: String,
    #[serde(default)]
    pub approved_provider_classes: Vec<ProviderBoundaryClass>,
    #[serde(default)]
    pub approved_provider_ids: Vec<String>,
    #[serde(default)]
    pub prohibited_provider_ids: Vec<String>,
    #[serde(default)]
    pub redact_classes: Vec<SensitiveDataClass>,
    #[serde(default)]
    pub tokenize_classes: Vec<SensitiveDataClass>,
    #[serde(default)]
    pub approval_required_classes: Vec<SensitiveDataClass>,
    #[serde(default)]
    pub block_classes: Vec<SensitiveDataClass>,
    #[serde(default)]
    pub action_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryInput {
    pub input_id: String,
    pub tenant: DataBoundaryTenantRef,
    pub provider: DataBoundaryProviderRef,
    pub operation: DataBoundaryOperationRef,
    pub payload_hash: String,
    pub payload_bytes: u64,
    #[serde(default)]
    pub source_refs: Vec<DataBoundaryEvidenceRef>,
    #[serde(default)]
    pub data_classes: Vec<SensitiveDataClass>,
    #[serde(default)]
    pub action_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryFinding {
    pub finding_id: String,
    pub data_class: SensitiveDataClass,
    pub severity: DataBoundaryFindingSeverity,
    pub confidence: u8,
    pub span_count: u32,
    #[serde(default)]
    pub sample_hashes: Vec<String>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<DataBoundaryEvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DataBoundaryFindingSummary {
    pub total_findings: u32,
    #[serde(default)]
    pub by_class: BTreeMap<SensitiveDataClass, u32>,
    #[serde(default)]
    pub by_severity: BTreeMap<DataBoundaryFindingSeverity, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryDecision {
    pub decision_id: String,
    pub mode: DataBoundaryMode,
    pub action: DataBoundaryAction,
    pub tenant: DataBoundaryTenantRef,
    pub provider: DataBoundaryProviderRef,
    pub operation: DataBoundaryOperationRef,
    pub payload_hash: String,
    pub policy_id: String,
    pub policy_fingerprint: String,
    pub finding_summary: DataBoundaryFindingSummary,
    #[serde(default)]
    pub finding_ids: Vec<String>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub action_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryEvent {
    pub event_id: String,
    pub event_name: String,
    pub event_kind: DataBoundaryEventKind,
    pub emitted_at_ms: u64,
    pub tenant: DataBoundaryTenantRef,
    pub provider: DataBoundaryProviderRef,
    pub operation: DataBoundaryOperationRef,
    pub decision_id: String,
    pub action: DataBoundaryAction,
    pub payload_hash: String,
    pub policy_fingerprint: String,
    pub finding_summary: DataBoundaryFindingSummary,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub action_tags: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<DataBoundaryEvidenceRef>,
}

impl DataBoundaryEvent {
    pub fn from_decision(
        event_id: impl Into<String>,
        event_kind: DataBoundaryEventKind,
        emitted_at_ms: u64,
        decision: &DataBoundaryDecision,
        evidence_refs: Vec<DataBoundaryEvidenceRef>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            event_name: event_kind.event_name().to_string(),
            event_kind,
            emitted_at_ms,
            tenant: decision.tenant.clone(),
            provider: decision.provider.clone(),
            operation: decision.operation.clone(),
            decision_id: decision.decision_id.clone(),
            action: decision.action,
            payload_hash: decision.payload_hash.clone(),
            policy_fingerprint: decision.policy_fingerprint.clone(),
            finding_summary: decision.finding_summary.clone(),
            reason_codes: decision.reason_codes.clone(),
            action_tags: decision.action_tags.clone(),
            evidence_refs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_decision() -> DataBoundaryDecision {
        let mut by_class = BTreeMap::new();
        by_class.insert(SensitiveDataClass::Credential, 1);
        let mut by_severity = BTreeMap::new();
        by_severity.insert(DataBoundaryFindingSeverity::High, 1);

        DataBoundaryDecision {
            decision_id: "dbd_001".to_string(),
            mode: DataBoundaryMode::Enforce,
            action: DataBoundaryAction::Block,
            tenant: DataBoundaryTenantRef {
                organization_id: Some("org_123".to_string()),
                workspace_id: Some("workspace_123".to_string()),
                deployment_id: Some("deployment_123".to_string()),
            },
            provider: DataBoundaryProviderRef {
                provider_id: "openai".to_string(),
                model_id: Some("gpt-5".to_string()),
                boundary_class: ProviderBoundaryClass::UnapprovedExternal,
            },
            operation: DataBoundaryOperationRef {
                operation_id: "op_123".to_string(),
                kind: DataBoundaryOperationKind::ProviderRequest,
                tool_name: None,
                source_ref: Some("runtime.context.pack".to_string()),
            },
            payload_hash: "sha256:payload".to_string(),
            policy_id: "default".to_string(),
            policy_fingerprint: "sha256:policy".to_string(),
            finding_summary: DataBoundaryFindingSummary {
                total_findings: 1,
                by_class,
                by_severity,
            },
            finding_ids: vec!["dbf_001".to_string()],
            reason_codes: vec!["credential_to_unapproved_external".to_string()],
            action_tags: vec!["provider_request".to_string(), "fail_closed".to_string()],
        }
    }

    #[test]
    fn event_names_match_contract() {
        assert_eq!(
            DataBoundaryEventKind::ApprovalRequired.event_name(),
            "data_boundary.approval_required"
        );
        assert_eq!(
            DataBoundaryEventKind::RoutedLocal.event_name(),
            "data_boundary.routed_local"
        );
    }

    #[test]
    fn decision_and_event_serialize_with_safe_refs() {
        let decision = sample_decision();
        let event = DataBoundaryEvent::from_decision(
            "dbe_001",
            DataBoundaryEventKind::Blocked,
            42,
            &decision,
            vec![DataBoundaryEvidenceRef {
                kind: DataBoundaryEvidenceKind::PayloadHash,
                ref_id: "payload".to_string(),
                path: None,
                hash: Some("sha256:payload".to_string()),
            }],
        );

        let value = serde_json::to_value(&event).expect("serialize event");
        assert_eq!(value["event_name"], "data_boundary.blocked");
        assert_eq!(value["payload_hash"], "sha256:payload");
        assert_eq!(value["policy_fingerprint"], "sha256:policy");
        assert_eq!(value["provider"]["boundary_class"], "unapproved_external");
        assert_eq!(value["finding_summary"]["by_class"]["credential"], 1);
    }

    #[test]
    fn event_contract_omits_raw_sensitive_payload_fields() {
        let decision = sample_decision();
        let event = DataBoundaryEvent::from_decision(
            "dbe_001",
            DataBoundaryEventKind::Blocked,
            42,
            &decision,
            Vec::new(),
        );

        let value = serde_json::to_value(&event).expect("serialize event");
        for forbidden in [
            "raw_payload",
            "raw_prompt",
            "prompt",
            "tool_result",
            "secret_value",
            "customer_data",
            "raw_model_output",
            "model_output",
        ] {
            assert!(
                value.get(forbidden).is_none(),
                "event must not expose raw field `{forbidden}`: {value:?}"
            );
        }
    }
}
