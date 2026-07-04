use std::collections::{BTreeMap, BTreeSet};

use crate::{
    detect_sensitive_data_with_config, payload_hash, redact_sensitive_data,
    tokenize_sensitive_data, DataBoundaryAction, DataBoundaryDecision, DataBoundaryDetectorConfig,
    DataBoundaryDetectorFinding, DataBoundaryEventKind, DataBoundaryFinding,
    DataBoundaryFindingSeverity, DataBoundaryFindingSummary, DataBoundaryInput, DataBoundaryMode,
    DataBoundaryPolicy, ProviderBoundaryClass, SensitiveDataClass,
};

/// Evaluation request wrapping a serializable [`DataBoundaryInput`] plus the
/// raw payload text, which is held transiently for detection/transformation
/// only. The raw payload never appears in the returned decision, findings, or
/// any serialized artifact — only hashes, classes, counts, and spans do.
pub struct DataBoundaryEvaluationRequest<'a> {
    pub input: &'a DataBoundaryInput,
    pub payload: Option<&'a str>,
    /// Detector tuning; `None` uses [`DataBoundaryDetectorConfig::default`].
    pub detector_config: Option<&'a DataBoundaryDetectorConfig>,
}

/// Result of a boundary evaluation. Deliberately not `Serialize`: the
/// `transformed_payload` still contains all non-sensitive payload text, so the
/// caller must forward it to the provider path only, never into audit or event
/// sinks. `decision` and `findings` are the audit-safe parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBoundaryEvaluation {
    pub decision: DataBoundaryDecision,
    pub findings: Vec<DataBoundaryFinding>,
    /// Redacted/tokenized payload when the action requires transformation and
    /// a raw payload was supplied; `None` otherwise.
    pub transformed_payload: Option<String>,
    /// Which `data_boundary.*` event this decision should emit.
    pub event_kind: DataBoundaryEventKind,
}

/// Pure decision function turning payload, provider class, tenant context, and
/// policy into an enforceable boundary decision.
///
/// * `Off` mode allows without running detection.
/// * `Audit` mode detects and may redact/tokenize per policy, but decisions
///   that would stop a call (block, approval, local routing) downgrade — to
///   the configured span-backed transformation when one applies, otherwise to
///   `AllowWithAudit` — with the original reason codes preserved.
/// * `Enforce` mode blocks prohibited providers, oversized payloads, strict
///   fail-closed violations, and raw sensitive data headed to unapproved
///   external providers unless policy explicitly allows the class.
/// * Redact/Tokenize apply only to detector-spanned classes; a class present
///   only as a declared hint cannot be located for transformation, so
///   transform policies on it fail closed rather than claiming a transform
///   that did not happen.
pub fn evaluate_data_boundary(
    request: &DataBoundaryEvaluationRequest<'_>,
    policy: &DataBoundaryPolicy,
) -> DataBoundaryEvaluation {
    let input = request.input;
    let resolved_payload_hash = resolve_payload_hash(input, request.payload);

    if policy.mode == DataBoundaryMode::Off {
        let decision = build_decision(
            input,
            policy,
            resolved_payload_hash,
            DataBoundaryAction::Allow,
            vec!["mode_off".to_string()],
            &[],
        );
        return DataBoundaryEvaluation {
            decision,
            findings: Vec::new(),
            transformed_payload: None,
            event_kind: DataBoundaryEventKind::Evaluated,
        };
    }

    let default_config = DataBoundaryDetectorConfig::default();
    let detector_config = request.detector_config.unwrap_or(&default_config);
    let detector_findings = request
        .payload
        .map(|payload| detect_sensitive_data_with_config(payload, detector_config))
        .unwrap_or_default();

    let findings = summarize_findings(&detector_findings);
    let detected_classes: BTreeSet<SensitiveDataClass> =
        findings.iter().map(|finding| finding.data_class).collect();
    let mut present_classes = detected_classes.clone();
    present_classes.extend(input.data_classes.iter().copied());

    let CandidateOutcome {
        action: candidate_action,
        mut reason_codes,
        transform_fallback,
    } = candidate_enforce_action(input, policy, &present_classes, &detected_classes);

    let action = if policy.mode == DataBoundaryMode::Audit && candidate_action.blocks_dispatch() {
        reason_codes.push("audit_mode_downgrade".to_string());
        // Audit mode never blocks, but a configured span-backed
        // redaction/tokenization still applies to the downgraded dispatch —
        // otherwise the downgrade would send raw content that policy
        // explicitly asked to transform.
        transform_fallback.unwrap_or(DataBoundaryAction::AllowWithAudit)
    } else {
        candidate_action
    };

    let transformed_payload = match (action, request.payload) {
        (DataBoundaryAction::Redact, Some(payload)) => {
            Some(redact_sensitive_data(payload, &detector_findings).redacted)
        }
        (DataBoundaryAction::Tokenize, Some(payload)) => {
            Some(tokenize_sensitive_data(payload, &detector_findings).tokenized)
        }
        _ => None,
    };

    let mut decision = build_decision(
        input,
        policy,
        resolved_payload_hash,
        action,
        reason_codes,
        &findings,
    );
    decision.finding_ids = findings.iter().map(|f| f.finding_id.clone()).collect();

    DataBoundaryEvaluation {
        decision,
        findings,
        transformed_payload,
        event_kind: event_kind_for_action(action),
    }
}

impl DataBoundaryAction {
    /// Actions that stop the original provider call from dispatching as-is.
    fn blocks_dispatch(self) -> bool {
        matches!(
            self,
            Self::Block | Self::RequireApproval | Self::RouteToLocal
        )
    }

    fn precedence(self) -> u8 {
        match self {
            Self::Allow => 0,
            Self::AllowWithAudit => 1,
            Self::Redact => 2,
            Self::Tokenize => 3,
            Self::RouteToLocal => 4,
            Self::RequireApproval => 5,
            Self::Block => 6,
        }
    }
}

fn resolve_payload_hash(input: &DataBoundaryInput, payload: Option<&str>) -> String {
    if !input.payload_hash.is_empty() {
        return input.payload_hash.clone();
    }
    payload
        .map(|payload| payload_hash(payload.as_bytes()))
        .unwrap_or_default()
}

fn provider_is_approved(input: &DataBoundaryInput, policy: &DataBoundaryPolicy) -> bool {
    let class = input.provider.boundary_class;
    if class.is_internal() || class == ProviderBoundaryClass::ApprovedExternal {
        return true;
    }
    if class == ProviderBoundaryClass::Prohibited {
        return false;
    }
    policy.approved_provider_classes.contains(&class)
        || policy
            .approved_provider_ids
            .contains(&input.provider.provider_id)
}

/// Outcome of the enforce-mode rule pass. `transform_fallback` is the
/// strongest span-backed Redact/Tokenize action that was triggered, kept
/// separately so an audit-mode downgrade of a blocking action can still apply
/// the transformation policy explicitly configured for the payload.
struct CandidateOutcome {
    action: DataBoundaryAction,
    reason_codes: Vec<String>,
    transform_fallback: Option<DataBoundaryAction>,
}

fn candidate_enforce_action(
    input: &DataBoundaryInput,
    policy: &DataBoundaryPolicy,
    present_classes: &BTreeSet<SensitiveDataClass>,
    detected_classes: &BTreeSet<SensitiveDataClass>,
) -> CandidateOutcome {
    let mut action = DataBoundaryAction::Allow;
    let mut reason_codes = Vec::new();
    let mut transform_fallback: Option<DataBoundaryAction> = None;
    fn raise(
        candidate: DataBoundaryAction,
        reason: String,
        action: &mut DataBoundaryAction,
        codes: &mut Vec<String>,
    ) {
        codes.push(reason);
        if candidate.precedence() > action.precedence() {
            *action = candidate;
        }
    }

    let provider_class = input.provider.boundary_class;
    if provider_class == ProviderBoundaryClass::Prohibited
        || policy
            .prohibited_provider_ids
            .contains(&input.provider.provider_id)
    {
        raise(
            DataBoundaryAction::Block,
            "prohibited_provider".to_string(),
            &mut action,
            &mut reason_codes,
        );
    }

    if policy.strict_fail_closed {
        let tenant = &input.tenant;
        if tenant.organization_id.is_none()
            && tenant.workspace_id.is_none()
            && tenant.deployment_id.is_none()
        {
            raise(
                DataBoundaryAction::Block,
                "missing_tenant_context".to_string(),
                &mut action,
                &mut reason_codes,
            );
        }
        if provider_class == ProviderBoundaryClass::Unknown {
            raise(
                DataBoundaryAction::Block,
                "unknown_provider_boundary_class".to_string(),
                &mut action,
                &mut reason_codes,
            );
        }
    }

    if let Some(max_bytes) = policy.max_payload_bytes {
        if input.payload_bytes > max_bytes {
            raise(
                DataBoundaryAction::Block,
                "payload_too_large".to_string(),
                &mut action,
                &mut reason_codes,
            );
        }
    }

    if present_classes.is_empty() {
        if reason_codes.is_empty() {
            reason_codes.push("no_findings".to_string());
        }
        return CandidateOutcome {
            action,
            reason_codes,
            transform_fallback,
        };
    }

    let approved_provider = provider_is_approved(input, policy);
    for class in present_classes {
        let label = class.placeholder_label().to_ascii_lowercase();
        if policy.block_classes.contains(class) {
            raise(
                DataBoundaryAction::Block,
                format!("blocked_class_{label}"),
                &mut action,
                &mut reason_codes,
            );
        }
        if policy.approval_required_classes.contains(class) {
            raise(
                DataBoundaryAction::RequireApproval,
                format!("approval_required_{label}"),
                &mut action,
                &mut reason_codes,
            );
        }
        if policy.require_local_classes.contains(class)
            && !input.provider.boundary_class.is_internal()
        {
            raise(
                DataBoundaryAction::RouteToLocal,
                format!("require_local_{label}"),
                &mut action,
                &mut reason_codes,
            );
        }
        if !approved_provider && !policy.allow_raw_external_classes.contains(class) {
            raise(
                DataBoundaryAction::Block,
                format!("raw_sensitive_to_unapproved_external_{label}"),
                &mut action,
                &mut reason_codes,
            );
        }
        // Redact/Tokenize can only be honored for classes the detector can
        // actually span. A class present only as a declared hint (e.g. a
        // governed memory/KB label) has no locatable content to transform, so
        // claiming Redact/Tokenize would dispatch it raw while the evidence
        // says otherwise — fail closed instead.
        let transformable = detected_classes.contains(class);
        for (configured, transform_action, kind) in [
            (
                &policy.tokenize_classes,
                DataBoundaryAction::Tokenize,
                "tokenize",
            ),
            (&policy.redact_classes, DataBoundaryAction::Redact, "redact"),
        ] {
            if !configured.contains(class) {
                continue;
            }
            if transformable {
                raise(
                    transform_action,
                    format!("{kind}_class_{label}"),
                    &mut action,
                    &mut reason_codes,
                );
                if transform_fallback.is_none_or(|f| transform_action.precedence() > f.precedence())
                {
                    transform_fallback = Some(transform_action);
                }
            } else {
                raise(
                    DataBoundaryAction::Block,
                    format!("untransformable_{kind}_class_{label}"),
                    &mut action,
                    &mut reason_codes,
                );
            }
        }
    }

    if action == DataBoundaryAction::Allow {
        raise(
            DataBoundaryAction::AllowWithAudit,
            "sensitive_classes_present".to_string(),
            &mut action,
            &mut reason_codes,
        );
    }

    CandidateOutcome {
        action,
        reason_codes,
        transform_fallback,
    }
}

/// Group span-level detector findings into one audit-safe
/// [`DataBoundaryFinding`] per sensitive class.
fn summarize_findings(
    detector_findings: &[DataBoundaryDetectorFinding],
) -> Vec<DataBoundaryFinding> {
    let mut grouped: BTreeMap<SensitiveDataClass, DataBoundaryFinding> = BTreeMap::new();
    for detected in detector_findings {
        let entry = grouped
            .entry(detected.data_class)
            .or_insert_with(|| DataBoundaryFinding {
                finding_id: format!(
                    "dbf_{}",
                    detected.data_class.placeholder_label().to_ascii_lowercase()
                ),
                data_class: detected.data_class,
                severity: detected.severity,
                confidence: detected.confidence,
                span_count: 0,
                sample_hashes: Vec::new(),
                reason_codes: Vec::new(),
                evidence_refs: Vec::new(),
            });
        entry.span_count += 1;
        entry.severity = entry.severity.max(detected.severity);
        entry.confidence = entry.confidence.max(detected.confidence);
        if entry.sample_hashes.len() < 3 && !entry.sample_hashes.contains(&detected.evidence_hash) {
            entry.sample_hashes.push(detected.evidence_hash.clone());
        }
        for code in &detected.reason_codes {
            if !entry.reason_codes.contains(code) {
                entry.reason_codes.push(code.clone());
            }
        }
    }
    grouped.into_values().collect()
}

fn build_decision(
    input: &DataBoundaryInput,
    policy: &DataBoundaryPolicy,
    payload_hash: String,
    action: DataBoundaryAction,
    reason_codes: Vec<String>,
    findings: &[DataBoundaryFinding],
) -> DataBoundaryDecision {
    let mut by_class = BTreeMap::new();
    let mut by_severity: BTreeMap<DataBoundaryFindingSeverity, u32> = BTreeMap::new();
    for finding in findings {
        by_class.insert(finding.data_class, finding.span_count);
        *by_severity.entry(finding.severity).or_default() += finding.span_count;
    }

    let mut action_tags = policy.action_tags.clone();
    for tag in &input.action_tags {
        if !action_tags.contains(tag) {
            action_tags.push(tag.clone());
        }
    }

    DataBoundaryDecision {
        decision_id: derive_decision_id(input, policy, &payload_hash),
        mode: policy.mode,
        action,
        tenant: input.tenant.clone(),
        provider: input.provider.clone(),
        operation: input.operation.clone(),
        payload_hash,
        policy_id: policy.policy_id.clone(),
        policy_fingerprint: policy.policy_fingerprint.clone(),
        finding_summary: DataBoundaryFindingSummary {
            total_findings: findings.iter().map(|f| f.span_count).sum(),
            by_class,
            by_severity,
        },
        finding_ids: Vec::new(),
        reason_codes,
        action_tags,
    }
}

/// Deterministic id so the same input under the same policy replays to the
/// same decision, keeping the function pure and retries dedupable.
fn derive_decision_id(
    input: &DataBoundaryInput,
    policy: &DataBoundaryPolicy,
    payload_hash: &str,
) -> String {
    let seed = format!(
        "{}|{}|{}|{}",
        input.input_id, input.operation.operation_id, policy.policy_fingerprint, payload_hash
    );
    let digest = crate::payload_hash(seed.as_bytes());
    let hex = digest.trim_start_matches("sha256:");
    format!("dbd_{}", &hex[..16.min(hex.len())])
}

fn event_kind_for_action(action: DataBoundaryAction) -> DataBoundaryEventKind {
    match action {
        DataBoundaryAction::Allow | DataBoundaryAction::AllowWithAudit => {
            DataBoundaryEventKind::Evaluated
        }
        DataBoundaryAction::Redact => DataBoundaryEventKind::Redacted,
        DataBoundaryAction::Tokenize => DataBoundaryEventKind::Tokenized,
        DataBoundaryAction::RouteToLocal => DataBoundaryEventKind::RoutedLocal,
        DataBoundaryAction::RequireApproval => DataBoundaryEventKind::ApprovalRequired,
        DataBoundaryAction::Block => DataBoundaryEventKind::Blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DataBoundaryOperationKind, DataBoundaryOperationRef, DataBoundaryProviderRef,
        DataBoundaryTenantRef,
    };

    fn tenant() -> DataBoundaryTenantRef {
        DataBoundaryTenantRef {
            organization_id: Some("org_1".to_string()),
            workspace_id: Some("ws_1".to_string()),
            deployment_id: None,
        }
    }

    fn input_for(class: ProviderBoundaryClass) -> DataBoundaryInput {
        DataBoundaryInput {
            input_id: "dbi_1".to_string(),
            tenant: tenant(),
            provider: DataBoundaryProviderRef {
                provider_id: "prov_1".to_string(),
                model_id: Some("model_1".to_string()),
                boundary_class: class,
            },
            operation: DataBoundaryOperationRef {
                operation_id: "op_1".to_string(),
                kind: DataBoundaryOperationKind::ProviderRequest,
                tool_name: None,
                source_ref: None,
            },
            payload_hash: String::new(),
            payload_bytes: 64,
            source_refs: Vec::new(),
            data_classes: Vec::new(),
            action_tags: Vec::new(),
        }
    }

    fn policy(mode: DataBoundaryMode) -> DataBoundaryPolicy {
        DataBoundaryPolicy {
            policy_id: "pol_1".to_string(),
            mode,
            policy_fingerprint: "sha256:policy".to_string(),
            approved_provider_classes: Vec::new(),
            approved_provider_ids: Vec::new(),
            prohibited_provider_ids: Vec::new(),
            redact_classes: Vec::new(),
            tokenize_classes: Vec::new(),
            approval_required_classes: Vec::new(),
            block_classes: Vec::new(),
            require_local_classes: Vec::new(),
            allow_raw_external_classes: Vec::new(),
            strict_fail_closed: false,
            max_payload_bytes: None,
            action_tags: Vec::new(),
        }
    }

    const SECRET_PAYLOAD: &str = "please use api_key=sk-live-abcdef1234567890 for the call";

    fn evaluate(
        input: &DataBoundaryInput,
        payload: Option<&str>,
        policy: &DataBoundaryPolicy,
    ) -> DataBoundaryEvaluation {
        evaluate_data_boundary(
            &DataBoundaryEvaluationRequest {
                input,
                payload,
                detector_config: None,
            },
            policy,
        )
    }

    #[test]
    fn off_mode_allows_without_findings() {
        let input = input_for(ProviderBoundaryClass::UnapprovedExternal);
        let result = evaluate(&input, Some(SECRET_PAYLOAD), &policy(DataBoundaryMode::Off));
        assert_eq!(result.decision.action, DataBoundaryAction::Allow);
        assert!(result.findings.is_empty());
        assert_eq!(result.decision.reason_codes, vec!["mode_off"]);
        assert_eq!(result.event_kind, DataBoundaryEventKind::Evaluated);
    }

    #[test]
    fn enforce_blocks_raw_sensitive_to_unapproved_external() {
        let input = input_for(ProviderBoundaryClass::UnapprovedExternal);
        let result = evaluate(
            &input,
            Some(SECRET_PAYLOAD),
            &policy(DataBoundaryMode::Enforce),
        );
        assert_eq!(result.decision.action, DataBoundaryAction::Block);
        assert_eq!(result.event_kind, DataBoundaryEventKind::Blocked);
        assert!(result
            .decision
            .reason_codes
            .iter()
            .any(|code| code.starts_with("raw_sensitive_to_unapproved_external_")));
    }

    #[test]
    fn enforce_allows_same_payload_to_local_provider() {
        let input = input_for(ProviderBoundaryClass::Local);
        let result = evaluate(
            &input,
            Some(SECRET_PAYLOAD),
            &policy(DataBoundaryMode::Enforce),
        );
        assert_eq!(result.decision.action, DataBoundaryAction::AllowWithAudit);
        assert!(!result.findings.is_empty());
    }

    #[test]
    fn allow_raw_external_class_permits_explicitly_allowed_class() {
        let input = input_for(ProviderBoundaryClass::UnapprovedExternal);
        let mut pol = policy(DataBoundaryMode::Enforce);
        // The payload detects as Credential plus a phone-like Pii span; every
        // present class must be explicitly allowed before raw egress passes.
        pol.allow_raw_external_classes = vec![SensitiveDataClass::Credential];
        let partial = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(partial.decision.action, DataBoundaryAction::Block);

        pol.allow_raw_external_classes =
            vec![SensitiveDataClass::Credential, SensitiveDataClass::Pii];
        let result = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::AllowWithAudit);
    }

    #[test]
    fn audit_mode_downgrades_block_to_allow_with_audit() {
        let input = input_for(ProviderBoundaryClass::UnapprovedExternal);
        let result = evaluate(
            &input,
            Some(SECRET_PAYLOAD),
            &policy(DataBoundaryMode::Audit),
        );
        assert_eq!(result.decision.action, DataBoundaryAction::AllowWithAudit);
        assert!(result
            .decision
            .reason_codes
            .iter()
            .any(|code| code == "audit_mode_downgrade"));
        assert!(!result.findings.is_empty());
    }

    #[test]
    fn audit_mode_still_redacts_configured_classes() {
        let input = input_for(ProviderBoundaryClass::ApprovedExternal);
        let mut pol = policy(DataBoundaryMode::Audit);
        pol.redact_classes = vec![SensitiveDataClass::Credential];
        let result = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::Redact);
        let transformed = result.transformed_payload.expect("redacted payload");
        assert!(!transformed.contains("sk-live-abcdef1234567890"));
        assert!(transformed.contains("[REDACTED:"));
        assert_eq!(result.event_kind, DataBoundaryEventKind::Redacted);
    }

    #[test]
    fn enforce_blocks_prohibited_provider_before_class_rules() {
        let input = input_for(ProviderBoundaryClass::Prohibited);
        let result = evaluate(&input, None, &policy(DataBoundaryMode::Enforce));
        assert_eq!(result.decision.action, DataBoundaryAction::Block);
        assert!(result
            .decision
            .reason_codes
            .contains(&"prohibited_provider".to_string()));
    }

    #[test]
    fn strict_fail_closed_blocks_missing_tenant_and_unknown_provider() {
        let mut input = input_for(ProviderBoundaryClass::Unknown);
        input.tenant = DataBoundaryTenantRef {
            organization_id: None,
            workspace_id: None,
            deployment_id: None,
        };
        let mut pol = policy(DataBoundaryMode::Enforce);
        pol.strict_fail_closed = true;
        let result = evaluate(&input, None, &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::Block);
        assert!(result
            .decision
            .reason_codes
            .contains(&"missing_tenant_context".to_string()));
        assert!(result
            .decision
            .reason_codes
            .contains(&"unknown_provider_boundary_class".to_string()));
    }

    #[test]
    fn non_strict_policy_does_not_fail_closed_on_missing_tenant() {
        let mut input = input_for(ProviderBoundaryClass::Local);
        input.tenant = DataBoundaryTenantRef {
            organization_id: None,
            workspace_id: None,
            deployment_id: None,
        };
        let result = evaluate(&input, None, &policy(DataBoundaryMode::Enforce));
        assert_eq!(result.decision.action, DataBoundaryAction::Allow);
    }

    #[test]
    fn oversized_payload_blocks_in_enforce_mode() {
        let mut input = input_for(ProviderBoundaryClass::Local);
        input.payload_bytes = 10_000;
        let mut pol = policy(DataBoundaryMode::Enforce);
        pol.max_payload_bytes = Some(1_000);
        let result = evaluate(&input, None, &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::Block);
        assert!(result
            .decision
            .reason_codes
            .contains(&"payload_too_large".to_string()));
    }

    #[test]
    fn approval_required_class_beats_redact() {
        let input = input_for(ProviderBoundaryClass::ApprovedExternal);
        let mut pol = policy(DataBoundaryMode::Enforce);
        pol.redact_classes = vec![SensitiveDataClass::Credential];
        pol.approval_required_classes = vec![SensitiveDataClass::Credential];
        let result = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::RequireApproval);
        assert_eq!(result.event_kind, DataBoundaryEventKind::ApprovalRequired);
        assert!(result.transformed_payload.is_none());
    }

    #[test]
    fn require_local_class_routes_external_provider_to_local() {
        let input = input_for(ProviderBoundaryClass::ApprovedExternal);
        let mut pol = policy(DataBoundaryMode::Enforce);
        pol.require_local_classes = vec![SensitiveDataClass::Credential];
        let result = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::RouteToLocal);
        assert_eq!(result.event_kind, DataBoundaryEventKind::RoutedLocal);
    }

    #[test]
    fn require_local_class_does_not_reroute_internal_provider() {
        let input = input_for(ProviderBoundaryClass::CustomerHosted);
        let mut pol = policy(DataBoundaryMode::Enforce);
        pol.require_local_classes = vec![SensitiveDataClass::Credential];
        let result = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::AllowWithAudit);
    }

    #[test]
    fn hint_only_transform_class_fails_closed_in_enforce() {
        // CustomerData arrives only as a declared hint (no detector span), so
        // a redact policy cannot actually transform it — claiming Redact
        // would dispatch it raw. Enforce mode must block instead.
        let mut input = input_for(ProviderBoundaryClass::ApprovedExternal);
        input.data_classes = vec![SensitiveDataClass::CustomerData];
        let mut pol = policy(DataBoundaryMode::Enforce);
        pol.redact_classes = vec![SensitiveDataClass::CustomerData];
        let result = evaluate(&input, Some("clean payload"), &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::Block);
        assert!(result.transformed_payload.is_none());
        assert!(result
            .decision
            .reason_codes
            .contains(&"untransformable_redact_class_customer_data".to_string()));

        // In audit mode the block downgrades, and with no span-backed
        // transformation available the honest action is AllowWithAudit —
        // never a false Redact claim.
        pol.mode = DataBoundaryMode::Audit;
        let audited = evaluate(&input, Some("clean payload"), &pol);
        assert_eq!(audited.decision.action, DataBoundaryAction::AllowWithAudit);
        assert!(audited.transformed_payload.is_none());
    }

    #[test]
    fn audit_downgrade_still_applies_configured_redaction() {
        // Raw-egress Block outranks Redact, but the audit downgrade must fall
        // back to the configured span-backed redaction rather than dispatch
        // the raw secret as plain AllowWithAudit.
        let input = input_for(ProviderBoundaryClass::UnapprovedExternal);
        let mut pol = policy(DataBoundaryMode::Audit);
        pol.redact_classes = vec![SensitiveDataClass::Credential];
        let result = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::Redact);
        assert_eq!(result.event_kind, DataBoundaryEventKind::Redacted);
        let transformed = result.transformed_payload.expect("redacted payload");
        assert!(!transformed.contains("sk-live-abcdef1234567890"));
        assert!(result
            .decision
            .reason_codes
            .contains(&"audit_mode_downgrade".to_string()));
        assert!(result
            .decision
            .reason_codes
            .iter()
            .any(|code| code.starts_with("raw_sensitive_to_unapproved_external_")));
    }

    #[test]
    fn input_data_class_hints_drive_policy_without_payload() {
        let mut input = input_for(ProviderBoundaryClass::UnapprovedExternal);
        input.data_classes = vec![SensitiveDataClass::CustomerData];
        let result = evaluate(&input, None, &policy(DataBoundaryMode::Enforce));
        assert_eq!(result.decision.action, DataBoundaryAction::Block);
    }

    #[test]
    fn decision_id_is_deterministic_for_same_input_and_policy() {
        let input = input_for(ProviderBoundaryClass::Local);
        let pol = policy(DataBoundaryMode::Audit);
        let first = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        let second = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(first.decision.decision_id, second.decision.decision_id);
        assert!(first.decision.decision_id.starts_with("dbd_"));
    }

    #[test]
    fn approved_provider_id_exempts_unapproved_external_class() {
        let input = input_for(ProviderBoundaryClass::UnapprovedExternal);
        let mut pol = policy(DataBoundaryMode::Enforce);
        pol.approved_provider_ids = vec!["prov_1".to_string()];
        let result = evaluate(&input, Some(SECRET_PAYLOAD), &pol);
        assert_eq!(result.decision.action, DataBoundaryAction::AllowWithAudit);
    }
}
