use super::*;

fn matrix() -> ApprovalGateMatrix {
    ApprovalGateMatrix::strict_default()
}

#[test]
fn external_customer_send_pauses_for_approval_by_default() {
    let outcome = matrix().resolve(&GateRequest::external_customer_send());
    assert!(outcome.requires_approval());
    assert_eq!(
        outcome.reason_code,
        "external_customer_send_requires_approval"
    );
    // A plain external send needs a human reviewer, not necessarily elevated.
    assert_eq!(
        outcome.reviewer_eligibility,
        ReviewerEligibility::AnyHumanReviewer
    );
    assert_eq!(outcome.approval_ttl_ms, DEFAULT_APPROVAL_TTL_MS);
}

#[test]
fn external_send_risk_tier_requires_approval() {
    let outcome = matrix().resolve(&GateRequest::new(Some(ToolRiskTier::ExternalSend), None));
    assert!(outcome.requires_approval());
    assert_eq!(
        outcome.reviewer_eligibility,
        ReviewerEligibility::AnyHumanReviewer
    );
}

#[test]
fn consequential_internal_write_requires_human_approval() {
    let outcome = matrix().resolve(&GateRequest::new(
        Some(ToolRiskTier::ConsequentialWrite),
        None,
    ));
    assert!(outcome.requires_approval());
    assert_eq!(
        outcome.reviewer_eligibility,
        ReviewerEligibility::AnyHumanReviewer
    );
}

#[test]
fn sensitive_data_classes_require_elevated_reviewer() {
    for class in [
        DataClass::Restricted,
        DataClass::Credential,
        DataClass::FinancialRecord,
        DataClass::Executive,
    ] {
        let outcome = matrix().resolve(&GateRequest::new(
            Some(ToolRiskTier::InternalWrite),
            Some(class),
        ));
        assert!(
            outcome.requires_approval(),
            "{class:?} should require approval"
        );
        assert_eq!(
            outcome.reviewer_eligibility,
            ReviewerEligibility::ElevatedReviewer,
            "{class:?} should require an elevated reviewer"
        );
        // Elevated gates use the tighter TTL window.
        assert_eq!(outcome.approval_ttl_ms, ELEVATED_APPROVAL_TTL_MS);
    }
}

#[test]
fn high_risk_tiers_require_elevated_reviewer() {
    for tier in [
        ToolRiskTier::FinancialRecordAccess,
        ToolRiskTier::CredentialAdmin,
        ToolRiskTier::DestructiveDelete,
        ToolRiskTier::MoneyMovementContract,
    ] {
        let outcome = matrix().resolve(&GateRequest::new(Some(tier), None));
        assert!(
            outcome.requires_approval(),
            "{tier:?} should require approval"
        );
        assert_eq!(
            outcome.reviewer_eligibility,
            ReviewerEligibility::ElevatedReviewer,
            "{tier:?} should require an elevated reviewer"
        );
    }
}

#[test]
fn low_risk_read_actions_auto_allow() {
    let outcome = matrix().resolve(&GateRequest::new(
        Some(ToolRiskTier::ReadDiscover),
        Some(DataClass::Internal),
    ));
    assert!(outcome.is_allowed());
    assert_eq!(outcome.reviewer_eligibility, ReviewerEligibility::None);
    assert_eq!(outcome.reason_code, "low_risk_auto_allow");
}

#[test]
fn internal_draft_without_sensitive_class_auto_allows() {
    let outcome = matrix().resolve(&GateRequest::new(Some(ToolRiskTier::ExternalDraft), None));
    assert!(outcome.is_allowed());
}

#[test]
fn unclassified_action_fails_closed_to_elevated_approval() {
    let outcome = matrix().resolve(&GateRequest::new(None, None));
    assert!(
        outcome.requires_approval(),
        "unknown action must not auto-allow"
    );
    assert_eq!(
        outcome.reviewer_eligibility,
        ReviewerEligibility::ElevatedReviewer
    );
    assert_eq!(outcome.reason_code, "unresolved_policy_fail_closed");
}

#[test]
fn categorically_denied_tier_is_denied() {
    let matrix = matrix().with_denied_risk_tiers(vec![ToolRiskTier::DestructiveDelete]);
    let outcome = matrix.resolve(&GateRequest::new(
        Some(ToolRiskTier::DestructiveDelete),
        None,
    ));
    assert!(outcome.is_denied());
    assert_eq!(outcome.reason_code, "risk_tier_blocked");
    assert_eq!(outcome.reviewer_eligibility, ReviewerEligibility::None);
}

#[test]
fn categorically_denied_data_class_is_denied() {
    let matrix = matrix().with_denied_data_classes(vec![DataClass::Credential]);
    let outcome = matrix.resolve(&GateRequest::new(
        Some(ToolRiskTier::CredentialAdmin),
        Some(DataClass::Credential),
    ));
    assert!(outcome.is_denied());
    assert_eq!(outcome.reason_code, "data_class_blocked");
}

#[test]
fn expired_approval_cannot_authorize_execution() {
    // Approved and still valid → authorizes.
    assert!(approval_authorizes_execution(true, 2_000, 1_500));
    // Approved but expired → cannot authorize.
    assert!(!approval_authorizes_execution(true, 2_000, 2_000));
    assert!(!approval_authorizes_execution(true, 2_000, 2_500));
    // Not approved → never authorizes, even before expiry.
    assert!(!approval_authorizes_execution(false, 2_000, 1_500));
}

#[test]
fn gate_outcome_round_trips_through_json() {
    let outcome = matrix().resolve(&GateRequest::external_customer_send());
    let encoded = serde_json::to_value(&outcome).expect("serialize outcome");
    assert_eq!(encoded["effect"], "approval_required");
    assert_eq!(encoded["reviewer_eligibility"], "any_human_reviewer");
    let decoded: GateOutcome = serde_json::from_value(encoded).expect("deserialize outcome");
    assert_eq!(decoded, outcome);
}

#[test]
fn matrix_round_trips_through_json() {
    let matrix = matrix().with_denied_risk_tiers(vec![ToolRiskTier::DestructiveDelete]);
    let encoded = serde_json::to_value(&matrix).expect("serialize matrix");
    let decoded: ApprovalGateMatrix = serde_json::from_value(encoded).expect("deserialize matrix");
    assert_eq!(decoded, matrix);
}
