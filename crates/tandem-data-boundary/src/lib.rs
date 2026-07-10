#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod evaluate;
mod provider_egress;

pub use evaluate::{evaluate_data_boundary, DataBoundaryEvaluation, DataBoundaryEvaluationRequest};
pub use provider_egress::{
    classify_provider_from_env, classify_provider_with, evaluate_provider_egress,
    evaluate_provider_egress_with_policy, provider_egress_mode_from_env, provider_egress_mode_with,
    provider_egress_payload_hash, provider_egress_policy_from_env, provider_egress_policy_with,
    BoundaryEnvLookup, ProviderEgressApproval, ProviderEgressAuditEvent, ProviderEgressAuthority,
    ProviderEgressDisposition, ProviderEgressEvaluation, ProviderEgressField, ProviderEgressPermit,
    ProviderEgressRequest,
};

/// Stable content hash for boundary inputs and monitoring dedupe. The result
/// is safe to log and never reveals payload content.
pub fn payload_hash(payload: &[u8]) -> String {
    sha256_hex(payload)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DataBoundaryMode {
    #[default]
    Off,
    Audit,
    Enforce,
}

impl DataBoundaryMode {
    /// Parses the `TANDEM_DATA_BOUNDARY_MODE` wire form. Returns `None` for
    /// unrecognized values so config validation can fail loudly instead of
    /// silently downgrading an intended `enforce` to `off`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "audit" => Some(Self::Audit),
            "enforce" => Some(Self::Enforce),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Audit => "audit",
            Self::Enforce => "enforce",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBoundaryClass {
    Local,
    CustomerHosted,
    ApprovedExternal,
    UnapprovedExternal,
    Prohibited,
    /// Caller could not classify the provider. Strict policies fail closed on
    /// this value; permissive policies treat it like `UnapprovedExternal`.
    Unknown,
}

impl ProviderBoundaryClass {
    /// Providers that keep payloads inside the company boundary.
    pub fn is_internal(self) -> bool {
        matches!(self, Self::Local | Self::CustomerHosted)
    }

    /// Parses the snake_case wire form used by provider-classification config
    /// (matching this enum's serde encoding). Returns `None` for unrecognized
    /// values so config validation can reject typos instead of silently
    /// classifying a provider more permissively than intended.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Self::Local),
            "customer_hosted" => Some(Self::CustomerHosted),
            "approved_external" => Some(Self::ApprovedExternal),
            "unapproved_external" => Some(Self::UnapprovedExternal),
            "prohibited" => Some(Self::Prohibited),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::CustomerHosted => "customer_hosted",
            Self::ApprovedExternal => "approved_external",
            Self::UnapprovedExternal => "unapproved_external",
            Self::Prohibited => "prohibited",
            Self::Unknown => "unknown",
        }
    }
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

impl SensitiveDataClass {
    /// Every sensitive class, for config surfaces that need "all classes".
    pub const ALL: [Self; 11] = [
        Self::Pii,
        Self::Phi,
        Self::Financial,
        Self::Credential,
        Self::Secret,
        Self::SourceCode,
        Self::CustomerData,
        Self::EmployeeData,
        Self::Legal,
        Self::ProprietaryBusinessData,
        Self::UnknownSensitive,
    ];

    /// Parses the snake_case wire form used by `TANDEM_DATA_BOUNDARY_*_CLASSES`
    /// lists (matching this enum's serde encoding). Returns `None` for
    /// unrecognized values so config validation can reject typos instead of
    /// silently dropping a class from a security policy.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pii" => Some(Self::Pii),
            "phi" => Some(Self::Phi),
            "financial" => Some(Self::Financial),
            "credential" => Some(Self::Credential),
            "secret" => Some(Self::Secret),
            "source_code" => Some(Self::SourceCode),
            "customer_data" => Some(Self::CustomerData),
            "employee_data" => Some(Self::EmployeeData),
            "legal" => Some(Self::Legal),
            "proprietary_business_data" => Some(Self::ProprietaryBusinessData),
            "unknown_sensitive" => Some(Self::UnknownSensitive),
            _ => None,
        }
    }

    pub fn placeholder_label(self) -> &'static str {
        match self {
            Self::Pii => "PII",
            Self::Phi => "PHI",
            Self::Financial => "FINANCIAL",
            Self::Credential => "CREDENTIAL",
            Self::Secret => "SECRET",
            Self::SourceCode => "SOURCE_CODE",
            Self::CustomerData => "CUSTOMER_DATA",
            Self::EmployeeData => "EMPLOYEE_DATA",
            Self::Legal => "LEGAL",
            Self::ProprietaryBusinessData => "PROPRIETARY_BUSINESS_DATA",
            Self::UnknownSensitive => "UNKNOWN_SENSITIVE",
        }
    }
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

impl DataBoundaryFindingSeverity {
    fn rank(self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
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
    /// Sensitive classes that must never leave for an external provider and
    /// should be routed to a local/private model instead.
    #[serde(default)]
    pub require_local_classes: Vec<SensitiveDataClass>,
    /// Sensitive classes policy explicitly allows to reach unapproved external
    /// providers in raw form. Empty means no raw sensitive data may cross.
    #[serde(default)]
    pub allow_raw_external_classes: Vec<SensitiveDataClass>,
    /// Fail closed (block in enforce mode) when tenant context is missing or
    /// the provider boundary class is `Unknown`.
    #[serde(default)]
    pub strict_fail_closed: bool,
    /// Payloads larger than this are refused in enforce mode to bound
    /// uncontrolled spend from payload/retry expansion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_payload_bytes: Option<u64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DataBoundarySpan {
    pub start: usize,
    pub end: usize,
}

impl DataBoundarySpan {
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryDetectorFinding {
    pub data_class: SensitiveDataClass,
    pub severity: DataBoundaryFindingSeverity,
    pub confidence: u8,
    pub span: DataBoundarySpan,
    pub detector_id: String,
    pub redaction_preview: String,
    pub evidence_hash: String,
    #[serde(default)]
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryDetectorConfig {
    pub detect_emails: bool,
    pub detect_phone_like: bool,
    pub detect_credit_cards: bool,
    pub detect_credentials: bool,
    pub detect_private_keys: bool,
    pub detect_aws_keys: bool,
    pub detect_high_entropy: bool,
    pub detect_simple_markers: bool,
}

impl Default for DataBoundaryDetectorConfig {
    fn default() -> Self {
        Self {
            detect_emails: true,
            detect_phone_like: true,
            detect_credit_cards: true,
            detect_credentials: true,
            detect_private_keys: true,
            detect_aws_keys: true,
            detect_high_entropy: true,
            detect_simple_markers: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryPlaceholder {
    pub placeholder: String,
    pub data_class: SensitiveDataClass,
    pub occurrence: u32,
    pub span: DataBoundarySpan,
    pub evidence_hash: String,
    pub detector_id: String,
    #[serde(default)]
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryRedaction {
    pub redacted: String,
    #[serde(default)]
    pub placeholders: Vec<DataBoundaryPlaceholder>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryTokenization {
    pub tokenized: String,
    #[serde(default)]
    pub placeholders: Vec<DataBoundaryPlaceholder>,
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
    /// How long the boundary evaluation took, for monitoring dispatch overhead.
    #[serde(default)]
    pub decision_latency_ms: u64,
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
        decision_latency_ms: u64,
        decision: &DataBoundaryDecision,
        evidence_refs: Vec<DataBoundaryEvidenceRef>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            event_name: event_kind.event_name().to_string(),
            event_kind,
            emitted_at_ms,
            decision_latency_ms,
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

pub fn detect_sensitive_data(input: &str) -> Vec<DataBoundaryDetectorFinding> {
    detect_sensitive_data_with_config(input, &DataBoundaryDetectorConfig::default())
}

pub fn detect_sensitive_data_with_config(
    input: &str,
    config: &DataBoundaryDetectorConfig,
) -> Vec<DataBoundaryDetectorFinding> {
    let mut findings = Vec::new();

    if config.detect_emails {
        detect_email_addresses(input, &mut findings);
    }
    if config.detect_phone_like {
        detect_phone_like_strings(input, &mut findings);
    }
    if config.detect_credit_cards {
        detect_credit_cards(input, &mut findings);
    }
    if config.detect_credentials {
        detect_credential_assignments(input, &mut findings);
        detect_bearer_tokens(input, &mut findings);
        detect_api_key_prefixes(input, &mut findings);
    }
    if config.detect_private_keys {
        detect_private_key_blocks(input, &mut findings);
    }
    if config.detect_aws_keys {
        detect_aws_access_keys(input, &mut findings);
    }
    if config.detect_high_entropy {
        detect_high_entropy_tokens(input, &mut findings);
    }
    if config.detect_simple_markers {
        detect_simple_sensitive_markers(input, &mut findings);
    }

    sort_and_dedupe_findings(&mut findings);
    findings
}

pub fn redact_sensitive_data(
    input: &str,
    findings: &[DataBoundaryDetectorFinding],
) -> DataBoundaryRedaction {
    let transformed = transform_sensitive_data(input, findings, "REDACTED");
    DataBoundaryRedaction {
        redacted: transformed.value,
        placeholders: transformed.placeholders,
    }
}

pub fn tokenize_sensitive_data(
    input: &str,
    findings: &[DataBoundaryDetectorFinding],
) -> DataBoundaryTokenization {
    let transformed = transform_sensitive_data(input, findings, "TOKEN");
    DataBoundaryTokenization {
        tokenized: transformed.value,
        placeholders: transformed.placeholders,
    }
}

pub fn detect_and_redact_sensitive_data(input: &str) -> DataBoundaryRedaction {
    let findings = detect_sensitive_data(input);
    redact_sensitive_data(input, &findings)
}

pub fn detect_and_tokenize_sensitive_data(input: &str) -> DataBoundaryTokenization {
    let findings = detect_sensitive_data(input);
    tokenize_sensitive_data(input, &findings)
}

struct DetectionSpec<'a> {
    data_class: SensitiveDataClass,
    severity: DataBoundaryFindingSeverity,
    confidence: u8,
    span: DataBoundarySpan,
    detector_id: &'a str,
    reason_codes: &'a [&'a str],
}

struct TransformOutput {
    value: String,
    placeholders: Vec<DataBoundaryPlaceholder>,
}

fn push_detection(
    findings: &mut Vec<DataBoundaryDetectorFinding>,
    input: &str,
    spec: DetectionSpec<'_>,
) {
    if !is_valid_span(input, spec.span) {
        return;
    }

    findings.push(DataBoundaryDetectorFinding {
        data_class: spec.data_class,
        severity: spec.severity,
        confidence: spec.confidence.min(100),
        span: spec.span,
        detector_id: spec.detector_id.to_string(),
        redaction_preview: format!("[REDACTED:{}]", spec.data_class.placeholder_label()),
        evidence_hash: hash_span(input, spec.span),
        reason_codes: spec
            .reason_codes
            .iter()
            .map(|reason| (*reason).to_string())
            .collect(),
    });
}

fn detect_email_addresses(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    let bytes = input.as_bytes();
    for at in bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'@').then_some(index))
    {
        let mut start = at;
        while start > 0 && is_email_local_byte(bytes[start - 1]) {
            start -= 1;
        }

        let mut end = at + 1;
        while end < bytes.len() && is_email_domain_byte(bytes[end]) {
            end += 1;
        }

        let span = DataBoundarySpan { start, end };
        if is_valid_email_candidate(&input[start..end]) {
            push_detection(
                findings,
                input,
                DetectionSpec {
                    data_class: SensitiveDataClass::Pii,
                    severity: DataBoundaryFindingSeverity::Medium,
                    confidence: 90,
                    span,
                    detector_id: "email_address",
                    reason_codes: &["email_address"],
                },
            );
        }
    }
}

fn detect_phone_like_strings(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if !is_phone_start_byte(bytes[index]) {
            index += 1;
            continue;
        }

        let start = index;
        let mut end = index;
        let mut digit_count = 0;
        let mut separator_count = 0;

        while end < bytes.len() && is_phone_body_byte(bytes[end]) {
            if bytes[end].is_ascii_digit() {
                digit_count += 1;
            } else {
                separator_count += 1;
            }
            end += 1;
        }

        while end > start && !bytes[end - 1].is_ascii_digit() {
            end -= 1;
        }

        if (10..=15).contains(&digit_count) && separator_count > 0 {
            push_detection(
                findings,
                input,
                DetectionSpec {
                    data_class: SensitiveDataClass::Pii,
                    severity: DataBoundaryFindingSeverity::Medium,
                    confidence: 70,
                    span: DataBoundarySpan { start, end },
                    detector_id: "phone_like",
                    reason_codes: &["phone_like"],
                },
            );
        }

        index = end.max(start + 1);
    }
}

fn detect_credit_cards(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }

        let start = index;
        let mut end = index;
        let mut digits = Vec::new();

        while end < bytes.len()
            && (bytes[end].is_ascii_digit() || bytes[end] == b' ' || bytes[end] == b'-')
        {
            if bytes[end].is_ascii_digit() {
                digits.push(bytes[end] - b'0');
            }
            end += 1;
        }

        while end > start && !bytes[end - 1].is_ascii_digit() {
            end -= 1;
        }

        if (13..=19).contains(&digits.len()) && luhn_valid(&digits) {
            push_detection(
                findings,
                input,
                DetectionSpec {
                    data_class: SensitiveDataClass::Financial,
                    severity: DataBoundaryFindingSeverity::High,
                    confidence: 95,
                    span: DataBoundarySpan { start, end },
                    detector_id: "credit_card_luhn",
                    reason_codes: &["credit_card_like", "luhn_valid"],
                },
            );
        }

        index = end.max(start + 1);
    }
}

fn detect_credential_assignments(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    let lower = input.to_ascii_lowercase();
    for key in [
        "access_token",
        "api_key",
        "apikey",
        "client_secret",
        "password",
        "passwd",
        "refresh_token",
        "secret_key",
        "secret",
        "token",
    ] {
        let mut offset = 0;
        while let Some(relative) = lower[offset..].find(key) {
            let key_start = offset + relative;
            let key_end = key_start + key.len();
            offset = key_end;

            if !has_identifier_boundary(lower.as_bytes(), key_start, key_end) {
                continue;
            }

            let Some((value_start, value_end)) = assignment_value_span(input, key_end) else {
                continue;
            };
            if value_end.saturating_sub(value_start) < 4 {
                continue;
            }

            let data_class = if key.contains("secret") {
                SensitiveDataClass::Secret
            } else {
                SensitiveDataClass::Credential
            };

            push_detection(
                findings,
                input,
                DetectionSpec {
                    data_class,
                    severity: DataBoundaryFindingSeverity::High,
                    confidence: 90,
                    span: DataBoundarySpan {
                        start: value_start,
                        end: value_end,
                    },
                    detector_id: "credential_assignment",
                    reason_codes: &["credential_assignment"],
                },
            );
        }
    }
}

fn detect_bearer_tokens(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    let lower = input.to_ascii_lowercase();
    let mut offset = 0;

    while let Some(relative) = lower[offset..].find("bearer ") {
        let start = offset + relative + "bearer ".len();
        let mut end = start;
        let bytes = input.as_bytes();

        while end < bytes.len() && is_secret_token_byte(bytes[end]) {
            end += 1;
        }
        offset = end.max(start + 1);

        if end.saturating_sub(start) < 8 {
            continue;
        }

        push_detection(
            findings,
            input,
            DetectionSpec {
                data_class: SensitiveDataClass::Credential,
                severity: DataBoundaryFindingSeverity::High,
                confidence: 95,
                span: DataBoundarySpan { start, end },
                detector_id: "bearer_token",
                reason_codes: &["bearer_token"],
            },
        );
    }
}

fn detect_api_key_prefixes(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    for prefix in [
        "ghp_",
        "gho_",
        "github_pat_",
        "pat_",
        "sk-",
        "xoxb-",
        "xoxp-",
    ] {
        let mut offset = 0;
        while let Some(relative) = input[offset..].find(prefix) {
            let start = offset + relative;
            let mut end = start + prefix.len();
            let bytes = input.as_bytes();
            offset = end;

            if start > 0 && is_secret_token_continuation_byte(bytes[start - 1]) {
                continue;
            }

            while end < bytes.len() && is_secret_token_byte(bytes[end]) {
                end += 1;
            }
            offset = end;

            if end.saturating_sub(start) < prefix.len() + 12 {
                continue;
            }

            push_detection(
                findings,
                input,
                DetectionSpec {
                    data_class: SensitiveDataClass::Credential,
                    severity: DataBoundaryFindingSeverity::High,
                    confidence: 85,
                    span: DataBoundarySpan { start, end },
                    detector_id: "api_key_prefix",
                    reason_codes: &["api_key_prefix"],
                },
            );
        }
    }
}

fn detect_private_key_blocks(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    let upper = input.to_ascii_uppercase();
    let mut offset = 0;

    while let Some(relative) = upper[offset..].find("-----BEGIN ") {
        let start = offset + relative;
        let search_start = start + "-----BEGIN ".len();
        offset = search_start;

        let Some(private_relative) = upper[search_start..].find("PRIVATE KEY-----") else {
            continue;
        };
        let marker_end = search_start + private_relative + "PRIVATE KEY-----".len();
        let mut end = marker_end;

        if let Some(end_relative) = upper[marker_end..].find("-----END ") {
            let end_start = marker_end + end_relative;
            if let Some(end_private_relative) = upper[end_start..].find("PRIVATE KEY-----") {
                end = end_start + end_private_relative + "PRIVATE KEY-----".len();
            }
        }
        offset = end;

        push_detection(
            findings,
            input,
            DetectionSpec {
                data_class: SensitiveDataClass::Secret,
                severity: DataBoundaryFindingSeverity::Critical,
                confidence: 99,
                span: DataBoundarySpan { start, end },
                detector_id: "private_key_block",
                reason_codes: &["private_key_block"],
            },
        );
    }
}

fn detect_aws_access_keys(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    const AWS_ACCESS_KEY_LEN: usize = 20;

    let bytes = input.as_bytes();
    if bytes.len() < AWS_ACCESS_KEY_LEN {
        return;
    }

    for start in 0..=bytes.len() - AWS_ACCESS_KEY_LEN {
        let candidate = &bytes[start..start + AWS_ACCESS_KEY_LEN];
        if !(candidate.starts_with(b"AKIA") || candidate.starts_with(b"ASIA")) {
            continue;
        }
        if !candidate
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            continue;
        }
        if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        if start + AWS_ACCESS_KEY_LEN < bytes.len()
            && bytes[start + AWS_ACCESS_KEY_LEN].is_ascii_alphanumeric()
        {
            continue;
        }

        push_detection(
            findings,
            input,
            DetectionSpec {
                data_class: SensitiveDataClass::Credential,
                severity: DataBoundaryFindingSeverity::High,
                confidence: 95,
                span: DataBoundarySpan {
                    start,
                    end: start + AWS_ACCESS_KEY_LEN,
                },
                detector_id: "aws_access_key_id",
                reason_codes: &["aws_access_key_id"],
            },
        );
    }
}

fn detect_high_entropy_tokens(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if !is_secret_token_byte(bytes[index]) {
            index += 1;
            continue;
        }

        let start = index;
        let mut end = index;
        while end < bytes.len() && is_secret_token_byte(bytes[end]) {
            end += 1;
        }
        index = end;

        let value_start = high_entropy_value_start(bytes, start, end);
        if end.saturating_sub(value_start) < 32 {
            continue;
        }

        let token = &input.as_bytes()[value_start..end];
        if looks_like_high_entropy_token(token) {
            push_detection(
                findings,
                input,
                DetectionSpec {
                    data_class: SensitiveDataClass::Secret,
                    severity: DataBoundaryFindingSeverity::Low,
                    confidence: 55,
                    span: DataBoundarySpan {
                        start: value_start,
                        end,
                    },
                    detector_id: "high_entropy_token",
                    reason_codes: &["high_entropy_token"],
                },
            );
        }
    }
}

fn detect_simple_sensitive_markers(input: &str, findings: &mut Vec<DataBoundaryDetectorFinding>) {
    let bytes = input.as_bytes();
    if bytes.len() < 11 {
        return;
    }

    for start in 0..=bytes.len() - 11 {
        let candidate = &bytes[start..start + 11];
        if is_ssn_like(candidate) {
            push_detection(
                findings,
                input,
                DetectionSpec {
                    data_class: SensitiveDataClass::Pii,
                    severity: DataBoundaryFindingSeverity::High,
                    confidence: 80,
                    span: DataBoundarySpan {
                        start,
                        end: start + 11,
                    },
                    detector_id: "ssn_like",
                    reason_codes: &["ssn_like"],
                },
            );
        }
    }

    let lower = input.to_ascii_lowercase();
    for marker in ["diagnosis:", "medical record:", "patient id:"] {
        let mut offset = 0;
        while let Some(relative) = lower[offset..].find(marker) {
            let marker_start = offset + relative;
            let value_start = marker_start + marker.len();
            offset = value_start;

            let Some((start, end)) = simple_marker_value_span(input, value_start) else {
                continue;
            };

            push_detection(
                findings,
                input,
                DetectionSpec {
                    data_class: SensitiveDataClass::Phi,
                    severity: DataBoundaryFindingSeverity::Medium,
                    confidence: 65,
                    span: DataBoundarySpan { start, end },
                    detector_id: "phi_marker",
                    reason_codes: &["phi_marker"],
                },
            );
        }
    }
}

fn transform_sensitive_data(
    input: &str,
    findings: &[DataBoundaryDetectorFinding],
    placeholder_prefix: &str,
) -> TransformOutput {
    let selected = select_transform_findings(input, findings);
    let mut value = String::with_capacity(input.len());
    let mut placeholders = Vec::with_capacity(selected.len());
    let mut cursor = 0;

    for finding in selected {
        value.push_str(&input[cursor..finding.span.start]);

        let occurrence = placeholders.len() as u32 + 1;
        let placeholder = format!(
            "[{}:{}:{}]",
            placeholder_prefix,
            finding.data_class.placeholder_label(),
            occurrence
        );
        value.push_str(&placeholder);
        placeholders.push(DataBoundaryPlaceholder {
            placeholder,
            data_class: finding.data_class,
            occurrence,
            span: finding.span,
            evidence_hash: finding.evidence_hash,
            detector_id: finding.detector_id,
            reason_codes: finding.reason_codes,
        });

        cursor = finding.span.end;
    }

    value.push_str(&input[cursor..]);
    TransformOutput {
        value,
        placeholders,
    }
}

fn select_transform_findings(
    input: &str,
    findings: &[DataBoundaryDetectorFinding],
) -> Vec<DataBoundaryDetectorFinding> {
    let mut candidates: Vec<_> = findings
        .iter()
        .filter(|finding| is_valid_span(input, finding.span))
        .collect();

    candidates.sort_by(|left, right| {
        left.span
            .start
            .cmp(&right.span.start)
            .then_with(|| transform_priority(right).cmp(&transform_priority(left)))
            .then_with(|| right.span.len().cmp(&left.span.len()))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| left.detector_id.cmp(&right.detector_id))
    });

    let mut selected: Vec<DataBoundaryDetectorFinding> = Vec::new();
    for candidate in candidates {
        if let Some(last) = selected.last_mut() {
            if candidate.span.start < last.span.end {
                merge_overlapping_transform_finding(input, last, candidate);
                continue;
            }
        }
        selected.push(candidate.clone());
    }

    selected.sort_by_key(|finding| finding.span.start);
    selected
}

fn merge_overlapping_transform_finding(
    input: &str,
    selected: &mut DataBoundaryDetectorFinding,
    candidate: &DataBoundaryDetectorFinding,
) {
    if candidate.span.start >= selected.span.start && candidate.span.end <= selected.span.end {
        return;
    }

    if candidate.span.start <= selected.span.start && candidate.span.end >= selected.span.end {
        *selected = candidate.clone();
        return;
    }

    let merged_span = DataBoundarySpan {
        start: selected.span.start.min(candidate.span.start),
        end: selected.span.end.max(candidate.span.end),
    };

    if should_replace_overlap(candidate, selected) {
        *selected = candidate.clone();
    }
    selected.span = merged_span;
    selected.evidence_hash = hash_span(input, merged_span);
}

fn should_replace_overlap(
    candidate: &DataBoundaryDetectorFinding,
    selected: &DataBoundaryDetectorFinding,
) -> bool {
    transform_priority(candidate)
        .cmp(&transform_priority(selected))
        .then_with(|| candidate.confidence.cmp(&selected.confidence))
        .then_with(|| candidate.span.len().cmp(&selected.span.len()))
        .then_with(|| selected.detector_id.cmp(&candidate.detector_id))
        .is_gt()
}

fn transform_priority(finding: &DataBoundaryDetectorFinding) -> u8 {
    let class_rank = match finding.data_class {
        SensitiveDataClass::Secret | SensitiveDataClass::Credential => 4,
        SensitiveDataClass::Financial | SensitiveDataClass::Phi => 3,
        SensitiveDataClass::Pii | SensitiveDataClass::Legal => 2,
        SensitiveDataClass::CustomerData
        | SensitiveDataClass::EmployeeData
        | SensitiveDataClass::ProprietaryBusinessData => 1,
        SensitiveDataClass::SourceCode | SensitiveDataClass::UnknownSensitive => 0,
    };
    finding.severity.rank() * 8 + class_rank
}

fn is_valid_span(input: &str, span: DataBoundarySpan) -> bool {
    !span.is_empty()
        && span.end <= input.len()
        && input.is_char_boundary(span.start)
        && input.is_char_boundary(span.end)
}

fn sort_and_dedupe_findings(findings: &mut Vec<DataBoundaryDetectorFinding>) {
    findings.sort_by(|left, right| {
        left.span
            .start
            .cmp(&right.span.start)
            .then_with(|| left.span.end.cmp(&right.span.end))
            .then_with(|| left.data_class.cmp(&right.data_class))
            .then_with(|| left.detector_id.cmp(&right.detector_id))
    });
    findings.dedup_by(|left, right| {
        left.span == right.span
            && left.data_class == right.data_class
            && left.detector_id == right.detector_id
    });
}

fn hash_span(input: &str, span: DataBoundarySpan) -> String {
    sha256_hex(&input.as_bytes()[span.start..span.end])
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn is_email_local_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_email_domain_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
}

fn is_valid_email_candidate(candidate: &str) -> bool {
    let Some((local, domain)) = candidate.split_once('@') else {
        return false;
    };
    if local.is_empty()
        || domain.len() < 4
        || local.starts_with('.')
        || local.ends_with('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || !domain.contains('.')
    {
        return false;
    }

    domain
        .rsplit('.')
        .next()
        .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|byte| byte.is_ascii_alphabetic()))
}

fn is_phone_start_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'+' | b'(')
}

fn is_phone_body_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b' ' | b'-' | b'.' | b'(' | b')')
}

fn is_secret_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+' | b'/' | b'=')
}

fn is_secret_token_continuation_byte(byte: u8) -> bool {
    is_secret_token_byte(byte) && byte != b'='
}

fn has_identifier_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_identifier_byte(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !is_identifier_byte(bytes[end]);
    before_ok && after_ok
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn high_entropy_value_start(bytes: &[u8], start: usize, end: usize) -> usize {
    let Some(relative) = bytes[start..end].iter().position(|byte| *byte == b'=') else {
        return start;
    };
    let equals = start + relative;
    if equals > start
        && bytes[start..equals]
            .iter()
            .all(|byte| is_identifier_byte(*byte))
    {
        equals + 1
    } else {
        start
    }
}

fn assignment_value_span(input: &str, key_end: usize) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut index = key_end;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if index >= bytes.len() || !matches!(bytes[index], b'=' | b':') {
        return None;
    }
    index += 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    let quote = if index < bytes.len() && matches!(bytes[index], b'\'' | b'"') {
        let quote = bytes[index];
        index += 1;
        Some(quote)
    } else {
        None
    };
    let start = index;

    let end = if let Some(quote) = quote {
        while index < bytes.len() {
            if bytes[index] == b'\\' && index + 1 < bytes.len() {
                index += 2;
                continue;
            }
            if bytes[index] == quote {
                break;
            }
            index += 1;
        }
        index
    } else {
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && !matches!(bytes[index], b',' | b';' | b'&')
        {
            index += 1;
        }
        index
    };

    (start < end).then_some((start, end))
}

fn simple_marker_value_span(input: &str, value_start: usize) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut start = value_start;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }

    let mut end = start;
    while end < bytes.len() && !matches!(bytes[end], b'\n' | b'\r' | b',' | b';') {
        end += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    (end.saturating_sub(start) >= 3).then_some((start, end))
}

fn luhn_valid(digits: &[u8]) -> bool {
    if digits.iter().all(|digit| *digit == 0) {
        return false;
    }

    let mut sum = 0u32;
    let mut double = false;
    for digit in digits.iter().rev() {
        let mut value = u32::from(*digit);
        if double {
            value *= 2;
            if value > 9 {
                value -= 9;
            }
        }
        sum += value;
        double = !double;
    }

    sum.is_multiple_of(10)
}

fn looks_like_high_entropy_token(token: &[u8]) -> bool {
    let mut unique = [false; 256];
    let mut unique_count = 0;
    let mut has_lower = false;
    let mut has_upper = false;
    let mut has_digit = false;
    let mut has_symbol = false;

    for byte in token {
        let index = usize::from(*byte);
        if !unique[index] {
            unique[index] = true;
            unique_count += 1;
        }
        has_lower |= byte.is_ascii_lowercase();
        has_upper |= byte.is_ascii_uppercase();
        has_digit |= byte.is_ascii_digit();
        has_symbol |= !byte.is_ascii_alphanumeric();
    }

    let class_count = [has_lower, has_upper, has_digit, has_symbol]
        .into_iter()
        .filter(|present| *present)
        .count();
    unique_count >= 12 && class_count >= 3
}

fn is_ssn_like(candidate: &[u8]) -> bool {
    candidate[0].is_ascii_digit()
        && candidate[1].is_ascii_digit()
        && candidate[2].is_ascii_digit()
        && candidate[3] == b'-'
        && candidate[4].is_ascii_digit()
        && candidate[5].is_ascii_digit()
        && candidate[6] == b'-'
        && candidate[7].is_ascii_digit()
        && candidate[8].is_ascii_digit()
        && candidate[9].is_ascii_digit()
        && candidate[10].is_ascii_digit()
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
            3,
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
            3,
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
