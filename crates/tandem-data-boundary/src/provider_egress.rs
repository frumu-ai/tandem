use std::borrow::Cow;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    detect_sensitive_data_with_config, payload_hash, redact_sensitive_data,
    tokenize_sensitive_data, DataBoundaryAction, DataBoundaryDetectorConfig,
    DataBoundaryDetectorFinding, DataBoundaryEvaluationRequest, DataBoundaryEvent,
    DataBoundaryEventKind, DataBoundaryInput, DataBoundaryMode, DataBoundaryOperationKind,
    DataBoundaryOperationRef, DataBoundaryPolicy, DataBoundaryProviderRef, DataBoundaryTenantRef,
    ProviderBoundaryClass, SensitiveDataClass,
};

/// Tenant and execution authority attached to one provider egress operation.
/// The optional authority reference is an opaque assertion/decision id, never
/// actor-provided content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderEgressAuthority {
    pub tenant: DataBoundaryTenantRef,
    #[serde(default, rename = "runID", skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, rename = "sessionID", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(
        default,
        rename = "authorityRef",
        skip_serializing_if = "Option::is_none"
    )]
    pub authority_ref: Option<String>,
}

impl ProviderEgressAuthority {
    pub fn new(tenant: DataBoundaryTenantRef) -> Self {
        Self {
            tenant,
            ..Self::default()
        }
    }

    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = non_empty(run_id.into());
        self
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = non_empty(session_id.into());
        self
    }

    pub fn with_authority_ref(mut self, authority_ref: impl Into<String>) -> Self {
        self.authority_ref = non_empty(authority_ref.into());
        self
    }

    fn has_run_ref(&self) -> bool {
        self.run_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    fn has_session_ref(&self) -> bool {
        self.session_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    fn has_organization_ref(&self) -> bool {
        self.tenant
            .organization_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    fn has_workspace_ref(&self) -> bool {
        self.tenant
            .workspace_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    fn has_complete_tenant_ref(&self) -> bool {
        self.has_organization_ref() && self.has_workspace_ref()
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

/// One independently transformable field in the payload sent to a provider.
/// Untransformable fields (for example signed attachment URLs) fail closed if
/// a redact/tokenize policy detects sensitive content in them.
#[derive(Debug, Clone)]
pub struct ProviderEgressField<'a> {
    pub label: Cow<'a, str>,
    pub value: Cow<'a, str>,
    pub binding_value: Option<Cow<'a, str>>,
    pub transformable: bool,
    pub detector_config: Option<DataBoundaryDetectorConfig>,
}

impl<'a> ProviderEgressField<'a> {
    pub fn transformable(label: impl Into<Cow<'a, str>>, value: impl Into<Cow<'a, str>>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            binding_value: None,
            transformable: true,
            detector_config: None,
        }
    }

    pub fn untransformable(label: impl Into<Cow<'a, str>>, value: impl Into<Cow<'a, str>>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            binding_value: None,
            transformable: false,
            detector_config: None,
        }
    }

    pub fn untransformable_with_detector_config(
        label: impl Into<Cow<'a, str>>,
        value: impl Into<Cow<'a, str>>,
        detector_config: DataBoundaryDetectorConfig,
    ) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            binding_value: None,
            transformable: false,
            detector_config: Some(detector_config),
        }
    }

    pub fn untransformable_with_binding(
        label: impl Into<Cow<'a, str>>,
        detector_value: impl Into<Cow<'a, str>>,
        binding_value: impl Into<Cow<'a, str>>,
    ) -> Self {
        Self {
            label: label.into(),
            value: detector_value.into(),
            binding_value: Some(binding_value.into()),
            transformable: false,
            detector_config: None,
        }
    }
}

/// Canonical request evaluated immediately before a provider dispatch.
pub struct ProviderEgressRequest<'a> {
    pub authority: &'a ProviderEgressAuthority,
    pub operation_id: &'a str,
    pub source_ref: &'a str,
    pub provider_id: &'a str,
    pub model_id: Option<&'a str>,
    pub fields: &'a [ProviderEgressField<'a>],
    pub data_classes: &'a [SensitiveDataClass],
    pub action_tags: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderEgressDisposition {
    Off,
    Proceed,
    RequireApproval,
    Blocked,
}

/// Opaque proof that the canonical boundary evaluated and authorized a
/// concrete provider/model route. Callers cannot construct permits directly;
/// provider dispatch APIs validate the evaluated route before network egress.
#[derive(Debug)]
pub struct ProviderEgressPermit {
    provider_id: String,
    model_id: Option<String>,
    operation_id: String,
    decision_id: String,
    approval_ref: Option<String>,
    payload_hash: String,
}

impl ProviderEgressPermit {
    pub fn ensure_request(
        &self,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        payload_hash: &str,
    ) -> Result<(), String> {
        let provider_id = provider_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                format!(
                    "DATA_BOUNDARY_PERMIT_INVALID: operation={} decision={} missing explicit provider route",
                    self.operation_id, self.decision_id
                )
            })?;
        if provider_id != self.provider_id {
            return Err(format!(
                "DATA_BOUNDARY_PERMIT_INVALID: operation={} decision={} provider route mismatch",
                self.operation_id, self.decision_id
            ));
        }
        let model_id = non_empty_ref(model_id);
        if model_id != self.model_id.as_deref() {
            return Err(format!(
                "DATA_BOUNDARY_PERMIT_INVALID: operation={} decision={} model route mismatch",
                self.operation_id, self.decision_id
            ));
        }
        if payload_hash != self.payload_hash {
            return Err(format!(
                "DATA_BOUNDARY_PERMIT_INVALID: operation={} decision={} payload mismatch",
                self.operation_id, self.decision_id
            ));
        }
        Ok(())
    }

    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }

    pub fn approval_ref(&self) -> Option<&str> {
        self.approval_ref.as_deref()
    }
}

/// Pending canonical permit that can only be activated with a non-empty
/// approval record reference supplied by the permission continuation.
#[derive(Debug)]
pub struct ProviderEgressApproval {
    provider_id: String,
    model_id: Option<String>,
    operation_id: String,
    decision_id: String,
    payload_hash: String,
}

impl ProviderEgressApproval {
    pub fn approve(self, approval_ref: impl Into<String>) -> Result<ProviderEgressPermit, String> {
        let approval_ref = non_empty(approval_ref.into()).ok_or_else(|| {
            "DATA_BOUNDARY_APPROVAL_INVALID: approval reference is required".to_string()
        })?;
        Ok(ProviderEgressPermit {
            provider_id: self.provider_id,
            model_id: self.model_id,
            operation_id: self.operation_id,
            decision_id: self.decision_id,
            approval_ref: Some(approval_ref),
            payload_hash: self.payload_hash,
        })
    }
}

fn non_empty_ref(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Audit-safe provider-egress event. Raw and transformed payloads never enter
/// this structure; tenant and execution attribution remain attached to the
/// boundary decision throughout dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderEgressAuditEvent {
    #[serde(flatten)]
    pub boundary: DataBoundaryEvent,
    #[serde(default, rename = "runID", skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, rename = "sessionID", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(
        default,
        rename = "authorityRef",
        skip_serializing_if = "Option::is_none"
    )]
    pub authority_ref: Option<String>,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(default, rename = "modelID", skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(rename = "sourceRef")]
    pub source_ref: String,
    #[serde(rename = "semanticDataClasses")]
    pub semantic_data_classes: Vec<SensitiveDataClass>,
    pub mode: String,
    #[serde(rename = "classificationSource")]
    pub classification_source: String,
    #[serde(rename = "auditOnly")]
    pub audit_only: bool,
    pub enforced: bool,
    #[serde(rename = "decidedEventKind")]
    pub decided_event_kind: String,
    #[serde(rename = "transformedFieldCount")]
    pub transformed_field_count: usize,
    #[serde(rename = "transformedSpans")]
    pub transformed_spans: usize,
}

#[derive(Debug)]
pub struct ProviderEgressEvaluation {
    pub mode: DataBoundaryMode,
    pub strict_fail_closed: bool,
    pub disposition: ProviderEgressDisposition,
    pub event: Option<ProviderEgressAuditEvent>,
    /// One output per input field when enforcement transformed the payload.
    /// `None` means the original fields must be dispatched unchanged.
    pub transformed_fields: Option<Vec<String>>,
    pub blocked_reason: Option<String>,
    permit: Option<ProviderEgressPermit>,
    approval: Option<ProviderEgressApproval>,
}

impl ProviderEgressEvaluation {
    pub fn take_dispatch_permit(&mut self) -> Result<ProviderEgressPermit, String> {
        self.permit.take().ok_or_else(|| {
            "DATA_BOUNDARY_PERMIT_UNAVAILABLE: dispatch was not authorized".to_string()
        })
    }

    pub fn take_approval(&mut self) -> Result<ProviderEgressApproval, String> {
        self.approval.take().ok_or_else(|| {
            "DATA_BOUNDARY_APPROVAL_UNAVAILABLE: dispatch is not awaiting approval".to_string()
        })
    }
}

/// Resolve environment policy and provider classification, then evaluate one
/// provider dispatch. Tests and tenant-policy adapters can call
/// [`evaluate_provider_egress_with_policy`] directly.
pub fn evaluate_provider_egress(request: &ProviderEgressRequest<'_>) -> ProviderEgressEvaluation {
    let mode = provider_egress_mode_from_env();
    let policy = provider_egress_policy_from_env(mode);
    let (boundary_class, classification_source) = classify_provider_from_env(request.provider_id);
    evaluate_provider_egress_with_policy(request, &policy, boundary_class, classification_source)
}

/// Configuration source for the `TANDEM_DATA_BOUNDARY_*` readers. The `_with`
/// variants let callers inject configuration (per-test, per-scope) instead of
/// reading the process environment, which is shared mutable state across every
/// concurrently running test in a process.
pub type BoundaryEnvLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn provider_egress_mode_from_env() -> DataBoundaryMode {
    provider_egress_mode_with(&process_env)
}

pub fn provider_egress_mode_with(lookup: BoundaryEnvLookup<'_>) -> DataBoundaryMode {
    lookup("TANDEM_DATA_BOUNDARY_MODE")
        .and_then(|raw| DataBoundaryMode::parse(&raw))
        .unwrap_or_default()
}

pub fn provider_egress_policy_from_env(mode: DataBoundaryMode) -> DataBoundaryPolicy {
    provider_egress_policy_with(mode, &process_env)
}

pub fn provider_egress_policy_with(
    mode: DataBoundaryMode,
    lookup: BoundaryEnvLookup<'_>,
) -> DataBoundaryPolicy {
    let mut policy = DataBoundaryPolicy {
        policy_id: "env".to_string(),
        mode,
        policy_fingerprint: String::new(),
        approved_provider_classes: Vec::new(),
        approved_provider_ids: Vec::new(),
        prohibited_provider_ids: Vec::new(),
        redact_classes: sensitive_class_list(lookup, "TANDEM_DATA_BOUNDARY_REDACT_CLASSES"),
        tokenize_classes: Vec::new(),
        approval_required_classes: sensitive_class_list(
            lookup,
            "TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES",
        ),
        block_classes: sensitive_class_list(lookup, "TANDEM_DATA_BOUNDARY_BLOCK_CLASSES"),
        require_local_classes: Vec::new(),
        allow_raw_external_classes: Vec::new(),
        strict_fail_closed: env_bool(lookup, "TANDEM_DATA_BOUNDARY_STRICT"),
        max_payload_bytes: lookup("TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES")
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|value| *value > 0),
        action_tags: Vec::new(),
    };
    apply_external_raw_policy(&mut policy, lookup);
    policy.policy_fingerprint = payload_hash(
        serde_json::to_string(&policy)
            .unwrap_or_default()
            .as_bytes(),
    );
    policy
}

fn env_bool(lookup: BoundaryEnvLookup<'_>, name: &str) -> bool {
    lookup(name)
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn sensitive_class_list(lookup: BoundaryEnvLookup<'_>, var: &str) -> Vec<SensitiveDataClass> {
    lookup(var)
        .map(|raw| {
            raw.split(',')
                .filter(|item| !item.trim().is_empty())
                .filter_map(SensitiveDataClass::parse)
                .collect()
        })
        .unwrap_or_default()
}

fn apply_external_raw_policy(policy: &mut DataBoundaryPolicy, lookup: BoundaryEnvLookup<'_>) {
    let Some(raw) = lookup("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY") else {
        return;
    };
    let all = SensitiveDataClass::ALL.to_vec();
    match raw.trim().to_ascii_lowercase().as_str() {
        "allow" | "audit" => policy.allow_raw_external_classes = all,
        "redact" => policy.redact_classes = all,
        "approval" => policy.approval_required_classes = all,
        "require_local" | "required_local" => policy.require_local_classes = all,
        _ => {}
    }
}

pub fn classify_provider_from_env(provider_id: &str) -> (ProviderBoundaryClass, &'static str) {
    classify_provider_with(provider_id, &process_env)
}

pub fn classify_provider_with(
    provider_id: &str,
    lookup: BoundaryEnvLookup<'_>,
) -> (ProviderBoundaryClass, &'static str) {
    if let Some(raw) = lookup("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES") {
        for entry in raw.split(',') {
            let Some((id, class)) = entry.split_once('=') else {
                continue;
            };
            if id.trim() == provider_id {
                if let Some(class) = ProviderBoundaryClass::parse(class) {
                    return (class, "env_mapping");
                }
            }
        }
    }
    (ProviderBoundaryClass::Unknown, "unclassified")
}

pub fn evaluate_provider_egress_with_policy(
    request: &ProviderEgressRequest<'_>,
    policy: &DataBoundaryPolicy,
    boundary_class: ProviderBoundaryClass,
    classification_source: &str,
) -> ProviderEgressEvaluation {
    if policy.mode == DataBoundaryMode::Off {
        return ProviderEgressEvaluation {
            mode: policy.mode,
            strict_fail_closed: policy.strict_fail_closed,
            disposition: ProviderEgressDisposition::Off,
            event: None,
            transformed_fields: None,
            blocked_reason: None,
            permit: Some(dispatch_permit(request, "mode_off", None, None)),
            approval: None,
        };
    }

    let started = Instant::now();
    let (payload, field_ranges) = assemble_payload(request.fields);
    let detector_findings = detect_fields(request.fields, &field_ranges);
    let input = DataBoundaryInput {
        input_id: format!("dbi_{}", request.operation_id),
        tenant: request.authority.tenant.clone(),
        provider: DataBoundaryProviderRef {
            provider_id: request.provider_id.to_string(),
            model_id: request.model_id.map(str::to_string),
            boundary_class,
        },
        operation: DataBoundaryOperationRef {
            operation_id: request.operation_id.to_string(),
            kind: DataBoundaryOperationKind::ProviderRequest,
            tool_name: None,
            source_ref: Some(request.source_ref.to_string()),
        },
        payload_hash: payload_hash(payload.as_bytes()),
        payload_bytes: payload.len() as u64,
        source_refs: Vec::new(),
        data_classes: request.data_classes.to_vec(),
        action_tags: request.action_tags.to_vec(),
    };
    let structured = crate::evaluate::evaluate_data_boundary_with_detector_findings(
        &DataBoundaryEvaluationRequest {
            input: &input,
            payload: Some(&payload),
            detector_config: None,
        },
        policy,
        detector_findings,
    );
    let mut evaluation = structured.evaluation;
    let detector_findings = structured.detector_findings;
    drop(payload);

    let decided_event_kind = evaluation.event_kind;
    evaluation.transformed_payload = None;
    if policy.mode == DataBoundaryMode::Enforce && policy.strict_fail_closed {
        if !request.authority.has_complete_tenant_ref()
            && !evaluation
                .decision
                .reason_codes
                .iter()
                .any(|code| code == "missing_tenant_context")
        {
            evaluation
                .decision
                .reason_codes
                .push("missing_tenant_context".to_string());
        }
        if !request.authority.has_organization_ref() {
            evaluation
                .decision
                .reason_codes
                .push("missing_tenant_organization".to_string());
        }
        if !request.authority.has_workspace_ref() {
            evaluation
                .decision
                .reason_codes
                .push("missing_tenant_workspace".to_string());
        }
        if !request.authority.has_run_ref() {
            evaluation
                .decision
                .reason_codes
                .push("missing_run_authority".to_string());
        }
        if !request.authority.has_session_ref() {
            evaluation
                .decision
                .reason_codes
                .push("missing_session_authority".to_string());
        }
        if !request.authority.has_run_ref() || !request.authority.has_session_ref() {
            evaluation
                .decision
                .reason_codes
                .push("missing_execution_authority".to_string());
        }
        if !request.authority.has_complete_tenant_ref()
            || !request.authority.has_run_ref()
            || !request.authority.has_session_ref()
        {
            evaluation.decision.action = DataBoundaryAction::Block;
            evaluation.event_kind = DataBoundaryEventKind::Blocked;
        }
    }

    let mut transformed_fields = None;
    let mut transformed_field_count = 0usize;
    let mut transformed_spans = 0usize;
    let mut disposition = ProviderEgressDisposition::Proceed;
    let mut event_kind = if policy.mode == DataBoundaryMode::Audit {
        DataBoundaryEventKind::Evaluated
    } else {
        evaluation.event_kind
    };
    let mut blocked_reason = None;

    if policy.mode == DataBoundaryMode::Enforce {
        match evaluation.decision.action {
            DataBoundaryAction::Allow | DataBoundaryAction::AllowWithAudit => {}
            DataBoundaryAction::Redact | DataBoundaryAction::Tokenize => {
                match transform_fields(
                    request.fields,
                    &field_ranges,
                    &detector_findings,
                    evaluation.decision.action,
                ) {
                    Ok(transformed) => {
                        transformed_field_count = transformed
                            .iter()
                            .zip(request.fields)
                            .filter(|(after, before)| after.as_str() != before.value.as_ref())
                            .count();
                        transformed_spans = detector_findings.len();
                        transformed_fields = Some(transformed);
                    }
                    Err(reason_code) => {
                        evaluation.decision.action = DataBoundaryAction::Block;
                        evaluation
                            .decision
                            .reason_codes
                            .push(reason_code.to_string());
                        event_kind = DataBoundaryEventKind::Blocked;
                        disposition = ProviderEgressDisposition::Blocked;
                    }
                }
            }
            DataBoundaryAction::RequireApproval => {
                disposition = ProviderEgressDisposition::RequireApproval;
            }
            DataBoundaryAction::RouteToLocal => {
                evaluation
                    .decision
                    .reason_codes
                    .push("route_to_local_unavailable".to_string());
                disposition = ProviderEgressDisposition::Blocked;
            }
            DataBoundaryAction::Block => {
                disposition = ProviderEgressDisposition::Blocked;
            }
        }
    }

    if matches!(
        disposition,
        ProviderEgressDisposition::Blocked | ProviderEgressDisposition::RequireApproval
    ) {
        let prefix = if disposition == ProviderEgressDisposition::RequireApproval {
            "DATA_BOUNDARY_APPROVAL_REQUIRED"
        } else {
            "DATA_BOUNDARY_BLOCKED"
        };
        blocked_reason = Some(audit_safe_reason(
            prefix,
            &evaluation.decision,
            request.data_classes,
        ));
    }

    let decision_id = evaluation.decision.decision_id.clone();

    let boundary_event = DataBoundaryEvent::from_decision(
        format!(
            "dbe_{}",
            evaluation.decision.decision_id.trim_start_matches("dbd_")
        ),
        event_kind,
        now_ms(),
        started.elapsed().as_millis() as u64,
        &evaluation.decision,
        Vec::new(),
    );
    let event = ProviderEgressAuditEvent {
        boundary: boundary_event,
        run_id: request.authority.run_id.clone(),
        session_id: request.authority.session_id.clone(),
        authority_ref: request.authority.authority_ref.clone(),
        provider_id: request.provider_id.to_string(),
        model_id: request.model_id.map(str::to_string),
        source_ref: request.source_ref.to_string(),
        semantic_data_classes: normalized_semantic_classes(request.data_classes),
        mode: policy.mode.as_str().to_string(),
        classification_source: classification_source.to_string(),
        audit_only: policy.mode == DataBoundaryMode::Audit,
        enforced: policy.mode == DataBoundaryMode::Enforce,
        decided_event_kind: decided_event_kind.event_name().to_string(),
        transformed_field_count,
        transformed_spans,
    };
    let (permit, approval) = match disposition {
        ProviderEgressDisposition::Proceed => (
            Some(dispatch_permit(
                request,
                &decision_id,
                None,
                transformed_fields.as_deref(),
            )),
            None,
        ),
        ProviderEgressDisposition::RequireApproval => (
            None,
            Some(ProviderEgressApproval {
                provider_id: request.provider_id.to_string(),
                model_id: non_empty_ref(request.model_id).map(str::to_string),
                operation_id: request.operation_id.to_string(),
                decision_id,
                payload_hash: provider_egress_payload_hash_with_values(request.fields, None),
            }),
        ),
        ProviderEgressDisposition::Off | ProviderEgressDisposition::Blocked => (None, None),
    };

    ProviderEgressEvaluation {
        mode: policy.mode,
        strict_fail_closed: policy.strict_fail_closed,
        disposition,
        event: Some(event),
        transformed_fields,
        blocked_reason,
        permit,
        approval,
    }
}

#[derive(Debug, Clone, Copy)]
struct FieldRange {
    start: usize,
    end: usize,
}

fn assemble_payload(fields: &[ProviderEgressField<'_>]) -> (String, Vec<FieldRange>) {
    let estimated = fields.iter().map(|field| field.value.len() + 1).sum();
    let mut payload = String::with_capacity(estimated);
    let mut ranges = Vec::with_capacity(fields.len());
    for field in fields {
        let start = payload.len();
        payload.push_str(field.value.as_ref());
        let end = payload.len();
        payload.push('\n');
        ranges.push(FieldRange { start, end });
    }
    (payload, ranges)
}

fn detect_fields(
    fields: &[ProviderEgressField<'_>],
    ranges: &[FieldRange],
) -> Vec<DataBoundaryDetectorFinding> {
    let default_config = DataBoundaryDetectorConfig::default();
    let mut findings = Vec::new();
    for (field, range) in fields.iter().zip(ranges) {
        let config = field.detector_config.as_ref().unwrap_or(&default_config);
        for mut finding in detect_sensitive_data_with_config(field.value.as_ref(), config) {
            finding.span.start += range.start;
            finding.span.end += range.start;
            findings.push(finding);
        }
    }
    findings
}

fn transform_fields(
    fields: &[ProviderEgressField<'_>],
    ranges: &[FieldRange],
    findings: &[DataBoundaryDetectorFinding],
    action: DataBoundaryAction,
) -> Result<Vec<String>, &'static str> {
    let mut by_field = vec![Vec::new(); fields.len()];
    for finding in findings {
        let Some((index, range)) = ranges
            .iter()
            .enumerate()
            .find(|(_, range)| finding.span.start >= range.start && finding.span.end <= range.end)
        else {
            return Err("sensitive_span_outside_payload_field");
        };
        if !fields[index].transformable {
            return Err("untransformable_sensitive_field");
        }
        let mut adjusted = finding.clone();
        adjusted.span.start -= range.start;
        adjusted.span.end -= range.start;
        by_field[index].push(adjusted);
    }

    Ok(fields
        .iter()
        .zip(by_field)
        .map(|(field, field_findings)| {
            if field_findings.is_empty() {
                return field.value.to_string();
            }
            if action == DataBoundaryAction::Tokenize {
                tokenize_sensitive_data(field.value.as_ref(), &field_findings).tokenized
            } else {
                redact_sensitive_data(field.value.as_ref(), &field_findings).redacted
            }
        })
        .collect())
}

fn normalized_semantic_classes(classes: &[SensitiveDataClass]) -> Vec<SensitiveDataClass> {
    let mut classes = classes.to_vec();
    classes.sort();
    classes.dedup();
    classes
}

fn dispatch_permit(
    request: &ProviderEgressRequest<'_>,
    decision_id: &str,
    approval_ref: Option<String>,
    transformed_fields: Option<&[String]>,
) -> ProviderEgressPermit {
    ProviderEgressPermit {
        provider_id: request.provider_id.to_string(),
        model_id: non_empty_ref(request.model_id).map(str::to_string),
        operation_id: request.operation_id.to_string(),
        decision_id: decision_id.to_string(),
        approval_ref,
        payload_hash: provider_egress_payload_hash_with_values(request.fields, transformed_fields),
    }
}

pub fn provider_egress_payload_hash(fields: &[ProviderEgressField<'_>]) -> String {
    provider_egress_payload_hash_with_values(fields, None)
}

fn provider_egress_payload_hash_with_values(
    fields: &[ProviderEgressField<'_>],
    transformed_fields: Option<&[String]>,
) -> String {
    let mut payload = String::new();
    for (index, field) in fields.iter().enumerate() {
        let value = field.binding_value.as_deref().unwrap_or_else(|| {
            transformed_fields
                .and_then(|values| values.get(index).map(String::as_str))
                .unwrap_or(field.value.as_ref())
        });
        payload.push_str(value);
        payload.push('\n');
    }
    payload_hash(payload.as_bytes())
}

fn audit_safe_reason(
    prefix: &str,
    decision: &crate::DataBoundaryDecision,
    semantic_classes: &[SensitiveDataClass],
) -> String {
    let mut classes = decision
        .finding_summary
        .by_class
        .keys()
        .copied()
        .collect::<Vec<_>>();
    classes.extend_from_slice(semantic_classes);
    classes.sort();
    classes.dedup();
    let classes = classes
        .into_iter()
        .map(SensitiveDataClass::placeholder_label)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{prefix}: provider={} classes=[{}] reason_codes=[{}]",
        decision.provider.provider_id,
        classes,
        decision.reason_codes.join(",")
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> ProviderEgressAuthority {
        ProviderEgressAuthority::new(DataBoundaryTenantRef {
            organization_id: Some("org-a".to_string()),
            workspace_id: Some("workspace-a".to_string()),
            deployment_id: None,
        })
        .with_run_id("run-a")
        .with_session_id("session-a")
        .with_authority_ref("assertion-a")
    }

    fn policy() -> DataBoundaryPolicy {
        DataBoundaryPolicy {
            policy_id: "test".to_string(),
            mode: DataBoundaryMode::Enforce,
            policy_fingerprint: "test-policy".to_string(),
            approved_provider_classes: vec![ProviderBoundaryClass::ApprovedExternal],
            approved_provider_ids: Vec::new(),
            prohibited_provider_ids: Vec::new(),
            redact_classes: vec![SensitiveDataClass::Pii, SensitiveDataClass::Credential],
            tokenize_classes: Vec::new(),
            approval_required_classes: Vec::new(),
            block_classes: Vec::new(),
            require_local_classes: Vec::new(),
            allow_raw_external_classes: Vec::new(),
            strict_fail_closed: true,
            max_payload_bytes: None,
            action_tags: Vec::new(),
        }
    }

    #[test]
    fn transforms_pii_and_secret_fields_from_one_evaluation() {
        let authority = authority();
        let fields = vec![
            ProviderEgressField::transformable("message.user", "email alice@example.com"),
            ProviderEgressField::transformable("message.tool", "api_key=sk-test-secret-123456"),
        ];
        let request = ProviderEgressRequest {
            authority: &authority,
            operation_id: "op-a",
            source_ref: "test.provider",
            provider_id: "openai",
            model_id: Some("gpt-test"),
            fields: &fields,
            data_classes: &[],
            action_tags: &[],
        };
        let result = evaluate_provider_egress_with_policy(
            &request,
            &policy(),
            ProviderBoundaryClass::ApprovedExternal,
            "test",
        );

        assert_eq!(result.disposition, ProviderEgressDisposition::Proceed);
        let transformed = result.transformed_fields.expect("transformed fields");
        assert!(!transformed.join("\n").contains("alice@example.com"));
        assert!(!transformed.join("\n").contains("sk-test-secret"));
        let event = result.event.expect("audit event");
        assert_eq!(event.transformed_field_count, 2);
        assert_eq!(event.run_id.as_deref(), Some("run-a"));
        assert_eq!(event.session_id.as_deref(), Some("session-a"));
        assert_eq!(event.authority_ref.as_deref(), Some("assertion-a"));
        let evidence = serde_json::to_string(&event).expect("serialize audit event");
        assert!(!evidence.contains("alice@example.com"));
        assert!(!evidence.contains("sk-test-secret"));
        let expected_spans = detect_sensitive_data_with_config(
            "email alice@example.com",
            &DataBoundaryDetectorConfig::default(),
        )
        .len()
            + detect_sensitive_data_with_config(
                "api_key=sk-test-secret-123456",
                &DataBoundaryDetectorConfig::default(),
            )
            .len();
        assert_eq!(event.transformed_spans, expected_spans);
    }

    #[test]
    fn strict_mode_blocks_missing_tenant_and_execution_authority() {
        let authority = ProviderEgressAuthority {
            tenant: DataBoundaryTenantRef {
                organization_id: Some("   ".to_string()),
                workspace_id: None,
                deployment_id: None,
            },
            run_id: Some("\t".to_string()),
            session_id: None,
            authority_ref: None,
        };
        let fields = vec![ProviderEgressField::transformable(
            "prompt",
            "ordinary text",
        )];
        let request = ProviderEgressRequest {
            authority: &authority,
            operation_id: "op-missing",
            source_ref: "test.provider",
            provider_id: "openai",
            model_id: None,
            fields: &fields,
            data_classes: &[],
            action_tags: &[],
        };
        let result = evaluate_provider_egress_with_policy(
            &request,
            &policy(),
            ProviderBoundaryClass::ApprovedExternal,
            "test",
        );

        assert_eq!(result.disposition, ProviderEgressDisposition::Blocked);
        let reason = result.blocked_reason.expect("blocked reason");
        assert!(reason.contains("missing_tenant_context"));
        assert!(reason.contains("missing_tenant_organization"));
        assert!(reason.contains("missing_tenant_workspace"));
        assert!(reason.contains("missing_run_authority"));
        assert!(reason.contains("missing_session_authority"));
        assert!(reason.contains("missing_execution_authority"));
    }

    #[test]
    fn semantic_classes_are_governed_without_detector_matches() {
        let authority = authority();
        let fields = vec![ProviderEgressField::transformable(
            "prompt",
            "ordinary words with no detector pattern",
        )];
        for data_class in [
            SensitiveDataClass::SourceCode,
            SensitiveDataClass::Legal,
            SensitiveDataClass::CustomerData,
            SensitiveDataClass::ProprietaryBusinessData,
            SensitiveDataClass::UnknownSensitive,
        ] {
            let request = ProviderEgressRequest {
                authority: &authority,
                operation_id: "op-semantic",
                source_ref: "test.semantic",
                provider_id: "openai",
                model_id: Some("gpt-test"),
                fields: &fields,
                data_classes: std::slice::from_ref(&data_class),
                action_tags: &[],
            };
            let mut blocked = policy();
            blocked.block_classes = vec![data_class];
            let result = evaluate_provider_egress_with_policy(
                &request,
                &blocked,
                ProviderBoundaryClass::ApprovedExternal,
                "test",
            );
            assert_eq!(result.disposition, ProviderEgressDisposition::Blocked);
            let event = result.event.expect("semantic audit event");
            assert_eq!(event.boundary.finding_summary.total_findings, 0);
            assert_eq!(event.semantic_data_classes, vec![data_class]);
            assert!(result
                .blocked_reason
                .expect("semantic block reason")
                .contains(data_class.placeholder_label()));
        }
    }

    #[test]
    fn permit_is_route_bound_and_approval_requires_a_reference() {
        let authority = authority();
        let fields = vec![ProviderEgressField::transformable(
            "prompt",
            "ordinary text",
        )];
        let request = ProviderEgressRequest {
            authority: &authority,
            operation_id: "op-permit",
            source_ref: "test.permit",
            provider_id: "openai",
            model_id: Some("gpt-test"),
            fields: &fields,
            data_classes: &[],
            action_tags: &[],
        };
        let mut allowed = evaluate_provider_egress_with_policy(
            &request,
            &policy(),
            ProviderBoundaryClass::ApprovedExternal,
            "test",
        );
        let permit = allowed.take_dispatch_permit().expect("dispatch permit");
        let payload_hash = provider_egress_payload_hash(&fields);
        assert!(permit
            .ensure_request(Some("openai"), Some("gpt-test"), &payload_hash)
            .is_ok());
        assert!(permit
            .ensure_request(Some("other"), Some("gpt-test"), &payload_hash)
            .is_err());
        let changed_fields = [ProviderEgressField::transformable(
            "prompt",
            "changed payload",
        )];
        assert!(permit
            .ensure_request(
                Some("openai"),
                Some("gpt-test"),
                &provider_egress_payload_hash(&changed_fields),
            )
            .is_err());

        let mut approval_policy = policy();
        approval_policy.approval_required_classes = vec![SensitiveDataClass::CustomerData];
        let approval_request = ProviderEgressRequest {
            data_classes: &[SensitiveDataClass::CustomerData],
            ..request
        };
        let mut pending = evaluate_provider_egress_with_policy(
            &approval_request,
            &approval_policy,
            ProviderBoundaryClass::ApprovedExternal,
            "test",
        );
        let approval = pending.take_approval().expect("pending approval");
        assert!(approval.approve(" ").is_err());

        let mut pending = evaluate_provider_egress_with_policy(
            &approval_request,
            &approval_policy,
            ProviderBoundaryClass::ApprovedExternal,
            "test",
        );
        let approved = pending
            .take_approval()
            .expect("pending approval")
            .approve("permission-1")
            .expect("activated permit");
        assert_eq!(approved.approval_ref(), Some("permission-1"));
    }

    #[test]
    fn strict_mode_blocks_unknown_provider_classification() {
        let authority = authority();
        let fields = vec![ProviderEgressField::transformable(
            "prompt",
            "ordinary text",
        )];
        let request = ProviderEgressRequest {
            authority: &authority,
            operation_id: "op-provider",
            source_ref: "test.provider",
            provider_id: "custom",
            model_id: None,
            fields: &fields,
            data_classes: &[],
            action_tags: &[],
        };
        let result = evaluate_provider_egress_with_policy(
            &request,
            &policy(),
            ProviderBoundaryClass::Unknown,
            "unclassified",
        );

        assert_eq!(result.disposition, ProviderEgressDisposition::Blocked);
        assert!(result
            .blocked_reason
            .expect("blocked reason")
            .contains("unknown_provider_boundary_class"));
    }

    #[test]
    fn sensitive_untransformable_field_fails_closed() {
        let authority = authority();
        let fields = vec![ProviderEgressField::untransformable(
            "attachment.url",
            "https://files.example.test/a?api_key=sk-test-secret-123456",
        )];
        let request = ProviderEgressRequest {
            authority: &authority,
            operation_id: "op-attachment",
            source_ref: "test.provider",
            provider_id: "openai",
            model_id: None,
            fields: &fields,
            data_classes: &[],
            action_tags: &[],
        };
        let result = evaluate_provider_egress_with_policy(
            &request,
            &policy(),
            ProviderBoundaryClass::ApprovedExternal,
            "test",
        );

        assert_eq!(result.disposition, ProviderEgressDisposition::Blocked);
        assert!(result
            .blocked_reason
            .expect("blocked reason")
            .contains("untransformable_sensitive_field"));
    }

    #[test]
    fn internal_field_labels_are_not_scanned_as_egress_content() {
        let authority = authority();
        let fields = vec![ProviderEgressField::transformable(
            "secret",
            "ordinary text",
        )];
        let request = ProviderEgressRequest {
            authority: &authority,
            operation_id: "op-label",
            source_ref: "test.provider",
            provider_id: "openai",
            model_id: None,
            fields: &fields,
            data_classes: &[],
            action_tags: &[],
        };
        let mut redact_all = policy();
        redact_all.redact_classes = SensitiveDataClass::ALL.to_vec();
        let result = evaluate_provider_egress_with_policy(
            &request,
            &redact_all,
            ProviderBoundaryClass::ApprovedExternal,
            "test",
        );

        assert_eq!(result.disposition, ProviderEgressDisposition::Proceed);
        assert!(result.transformed_fields.is_none());
        assert_eq!(
            result
                .event
                .expect("audit event")
                .boundary
                .finding_summary
                .total_findings,
            0
        );
    }

    #[test]
    fn audit_mode_preserves_original_fields() {
        let authority = authority();
        let fields = vec![ProviderEgressField::transformable(
            "prompt",
            "email alice@example.com",
        )];
        let request = ProviderEgressRequest {
            authority: &authority,
            operation_id: "op-audit",
            source_ref: "test.provider",
            provider_id: "openai",
            model_id: None,
            fields: &fields,
            data_classes: &[],
            action_tags: &[],
        };
        let mut audit_policy = policy();
        audit_policy.mode = DataBoundaryMode::Audit;
        let result = evaluate_provider_egress_with_policy(
            &request,
            &audit_policy,
            ProviderBoundaryClass::ApprovedExternal,
            "test",
        );

        assert_eq!(result.disposition, ProviderEgressDisposition::Proceed);
        assert!(result.transformed_fields.is_none());
        let event = result.event.expect("audit event");
        assert!(event.audit_only);
        assert_eq!(event.boundary.event_kind, DataBoundaryEventKind::Evaluated);
    }
}
