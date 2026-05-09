use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfile {
    #[default]
    Strict,
    Guided,
    Yolo,
}

impl ExecutionProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionProfile::Strict => "strict",
            ExecutionProfile::Guided => "guided",
            ExecutionProfile::Yolo => "yolo",
        }
    }

    pub fn allows_validation_warning(self) -> bool {
        matches!(self, ExecutionProfile::Guided | ExecutionProfile::Yolo)
    }

    pub fn allows_experimental_continue(self) -> bool {
        matches!(self, ExecutionProfile::Yolo)
    }

    pub fn repair_budget_multiplier(self) -> f32 {
        match self {
            ExecutionProfile::Strict => 1.0,
            ExecutionProfile::Guided => 1.5,
            ExecutionProfile::Yolo => 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorClass {
    MissingRequiredSection,
    WeakMarkdownStructure,
    MissingOptionalEvidence,
    ArtifactWordCountBelowMinimum,
    MissingNonconsumedWorkspaceFiles,
    MissingRequiredArtifactPath,
    ValidatorKindSpecificSoftCheck,
    RepairBudgetExhausted,
    UnauthorizedWorkspace,
    SecretAccessDenied,
    DestructiveActionRequiresApproval,
    ExternalPublishRequiresApproval,
    TenantPolicyDenied,
    ToolUnauthorized,
    BudgetExceeded,
    KillSwitchEngaged,
    EngineLeaseExpired,
    InvalidApiToken,
    DeterministicVerificationFailed,
}

impl ValidatorClass {
    pub fn is_critical(self) -> bool {
        matches!(
            self,
            ValidatorClass::UnauthorizedWorkspace
                | ValidatorClass::SecretAccessDenied
                | ValidatorClass::DestructiveActionRequiresApproval
                | ValidatorClass::ExternalPublishRequiresApproval
                | ValidatorClass::TenantPolicyDenied
                | ValidatorClass::ToolUnauthorized
                | ValidatorClass::BudgetExceeded
                | ValidatorClass::KillSwitchEngaged
                | ValidatorClass::EngineLeaseExpired
                | ValidatorClass::InvalidApiToken
                | ValidatorClass::DeterministicVerificationFailed
        )
    }

    pub fn is_relaxable_in(self, profile: ExecutionProfile) -> bool {
        if self.is_critical() {
            return false;
        }
        match (self, profile) {
            (_, ExecutionProfile::Strict) => false,
            (
                ValidatorClass::MissingRequiredSection
                | ValidatorClass::WeakMarkdownStructure
                | ValidatorClass::MissingOptionalEvidence
                | ValidatorClass::ArtifactWordCountBelowMinimum
                | ValidatorClass::MissingNonconsumedWorkspaceFiles,
                ExecutionProfile::Guided | ExecutionProfile::Yolo,
            ) => true,
            (
                ValidatorClass::MissingRequiredArtifactPath
                | ValidatorClass::ValidatorKindSpecificSoftCheck
                | ValidatorClass::RepairBudgetExhausted,
                ExecutionProfile::Yolo,
            ) => true,
            _ => false,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ValidatorClass::MissingRequiredSection => "missing_required_section",
            ValidatorClass::WeakMarkdownStructure => "weak_markdown_structure",
            ValidatorClass::MissingOptionalEvidence => "missing_optional_evidence",
            ValidatorClass::ArtifactWordCountBelowMinimum => "artifact_word_count_below_minimum",
            ValidatorClass::MissingNonconsumedWorkspaceFiles => {
                "missing_nonconsumed_workspace_files"
            }
            ValidatorClass::MissingRequiredArtifactPath => "missing_required_artifact_path",
            ValidatorClass::ValidatorKindSpecificSoftCheck => "validator_kind_specific_soft_check",
            ValidatorClass::RepairBudgetExhausted => "repair_budget_exhausted",
            ValidatorClass::UnauthorizedWorkspace => "unauthorized_workspace",
            ValidatorClass::SecretAccessDenied => "secret_access_denied",
            ValidatorClass::DestructiveActionRequiresApproval => {
                "destructive_action_requires_approval"
            }
            ValidatorClass::ExternalPublishRequiresApproval => "external_publish_requires_approval",
            ValidatorClass::TenantPolicyDenied => "tenant_policy_denied",
            ValidatorClass::ToolUnauthorized => "tool_unauthorized",
            ValidatorClass::BudgetExceeded => "budget_exceeded",
            ValidatorClass::KillSwitchEngaged => "kill_switch_engaged",
            ValidatorClass::EngineLeaseExpired => "engine_lease_expired",
            ValidatorClass::InvalidApiToken => "invalid_api_token",
            ValidatorClass::DeterministicVerificationFailed => "deterministic_verification_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ValidationOutcome {
    Passed,
    Warning,
    Experimental,
    Blocked,
}

impl ValidationOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ValidationOutcome::Passed => "passed",
            ValidationOutcome::Warning => "warning",
            ValidationOutcome::Experimental => "experimental",
            ValidationOutcome::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelaxedValidatorClass {
    pub class: ValidatorClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub original_outcome: ValidationOutcome,
    pub effective_outcome: ValidationOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileValidationDecision {
    pub profile: ExecutionProfile,
    pub original_outcome: ValidationOutcome,
    pub effective_outcome: ValidationOutcome,
    pub should_block: bool,
    pub experimental: bool,
    pub relaxed_classes: Vec<RelaxedValidatorClass>,
}

impl ProfileValidationDecision {
    pub fn passthrough(profile: ExecutionProfile, outcome: ValidationOutcome) -> Self {
        ProfileValidationDecision {
            profile,
            original_outcome: outcome,
            effective_outcome: outcome,
            should_block: matches!(outcome, ValidationOutcome::Blocked),
            experimental: false,
            relaxed_classes: Vec::new(),
        }
    }
}

/// Single chokepoint: given a non-pass validator outcome and the validator
/// classes that triggered it, decide what the run/node should actually see
/// under the active profile. All profile-driven downgrades MUST flow through
/// this function — see `docs/internal/execution-profiles/PROPOSAL.md`
/// "Executor Chokepoint Invariant".
pub fn decide_profile_validation(
    profile: ExecutionProfile,
    original_outcome: ValidationOutcome,
    classes: &[(ValidatorClass, Option<String>)],
    tenant_relaxation_denylist: &[ValidatorClass],
) -> ProfileValidationDecision {
    if matches!(
        original_outcome,
        ValidationOutcome::Passed | ValidationOutcome::Warning
    ) {
        return ProfileValidationDecision::passthrough(profile, original_outcome);
    }

    if classes.is_empty() {
        return ProfileValidationDecision::passthrough(profile, original_outcome);
    }

    let any_critical = classes.iter().any(|(class, _)| class.is_critical());
    if any_critical {
        return ProfileValidationDecision::passthrough(profile, ValidationOutcome::Blocked);
    }

    let any_tenant_denied = classes
        .iter()
        .any(|(class, _)| tenant_relaxation_denylist.contains(class));
    if any_tenant_denied {
        return ProfileValidationDecision::passthrough(profile, ValidationOutcome::Blocked);
    }

    let all_relaxable = classes
        .iter()
        .all(|(class, _)| class.is_relaxable_in(profile));
    if !all_relaxable {
        return ProfileValidationDecision::passthrough(profile, ValidationOutcome::Blocked);
    }

    let effective_outcome = match profile {
        ExecutionProfile::Strict => ValidationOutcome::Blocked,
        ExecutionProfile::Guided => ValidationOutcome::Warning,
        ExecutionProfile::Yolo => ValidationOutcome::Experimental,
    };

    let relaxed_classes = classes
        .iter()
        .map(|(class, detail)| RelaxedValidatorClass {
            class: *class,
            detail: detail.clone(),
            original_outcome,
            effective_outcome,
        })
        .collect();

    ProfileValidationDecision {
        profile,
        original_outcome,
        effective_outcome,
        should_block: matches!(effective_outcome, ValidationOutcome::Blocked),
        experimental: matches!(effective_outcome, ValidationOutcome::Experimental),
        relaxed_classes,
    }
}

/// Profile-aware repair budget multiplier, bounded above by global caps in
/// `AutomationExecutionPolicy`. Returns the effective number of repair
/// attempts allowed for the given declared budget under `profile`.
pub fn effective_repair_budget(declared: u32, profile: ExecutionProfile) -> u32 {
    let multiplier = profile.repair_budget_multiplier();
    let scaled = (declared as f32 * multiplier).ceil();
    scaled.clamp(0.0, u32::MAX as f32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_profile_serde_round_trip() {
        for (profile, wire) in [
            (ExecutionProfile::Strict, "\"strict\""),
            (ExecutionProfile::Guided, "\"guided\""),
            (ExecutionProfile::Yolo, "\"yolo\""),
        ] {
            let serialized = serde_json::to_string(&profile).unwrap();
            assert_eq!(serialized, wire);
            let deserialized: ExecutionProfile = serde_json::from_str(wire).unwrap();
            assert_eq!(deserialized, profile);
        }
    }

    #[test]
    fn execution_profile_default_is_strict() {
        assert_eq!(ExecutionProfile::default(), ExecutionProfile::Strict);
    }

    #[test]
    fn execution_profile_unknown_string_fails() {
        assert!(serde_json::from_str::<ExecutionProfile>("\"loose\"").is_err());
    }

    #[test]
    fn critical_classes_never_relaxable() {
        let critical = [
            ValidatorClass::UnauthorizedWorkspace,
            ValidatorClass::SecretAccessDenied,
            ValidatorClass::DestructiveActionRequiresApproval,
            ValidatorClass::TenantPolicyDenied,
            ValidatorClass::ToolUnauthorized,
            ValidatorClass::BudgetExceeded,
            ValidatorClass::KillSwitchEngaged,
            ValidatorClass::DeterministicVerificationFailed,
        ];
        for class in critical {
            assert!(class.is_critical(), "{:?} should be critical", class);
            for profile in [
                ExecutionProfile::Strict,
                ExecutionProfile::Guided,
                ExecutionProfile::Yolo,
            ] {
                assert!(
                    !class.is_relaxable_in(profile),
                    "{:?} must not be relaxable in {:?}",
                    class,
                    profile
                );
            }
        }
    }

    #[test]
    fn guided_relaxes_soft_classes() {
        let soft = [
            ValidatorClass::MissingRequiredSection,
            ValidatorClass::WeakMarkdownStructure,
            ValidatorClass::MissingOptionalEvidence,
            ValidatorClass::ArtifactWordCountBelowMinimum,
            ValidatorClass::MissingNonconsumedWorkspaceFiles,
        ];
        for class in soft {
            assert!(class.is_relaxable_in(ExecutionProfile::Guided));
            assert!(class.is_relaxable_in(ExecutionProfile::Yolo));
            assert!(!class.is_relaxable_in(ExecutionProfile::Strict));
        }
    }

    #[test]
    fn yolo_only_classes_not_relaxed_in_guided() {
        let yolo_only = [
            ValidatorClass::MissingRequiredArtifactPath,
            ValidatorClass::ValidatorKindSpecificSoftCheck,
            ValidatorClass::RepairBudgetExhausted,
        ];
        for class in yolo_only {
            assert!(!class.is_relaxable_in(ExecutionProfile::Strict));
            assert!(!class.is_relaxable_in(ExecutionProfile::Guided));
            assert!(class.is_relaxable_in(ExecutionProfile::Yolo));
        }
    }

    #[test]
    fn decide_blocked_under_strict_stays_blocked() {
        let decision = decide_profile_validation(
            ExecutionProfile::Strict,
            ValidationOutcome::Blocked,
            &[(
                ValidatorClass::MissingRequiredSection,
                Some("Sources".into()),
            )],
            &[],
        );
        assert!(decision.should_block);
        assert_eq!(decision.effective_outcome, ValidationOutcome::Blocked);
        assert!(decision.relaxed_classes.is_empty());
    }

    #[test]
    fn decide_soft_under_guided_becomes_warning() {
        let decision = decide_profile_validation(
            ExecutionProfile::Guided,
            ValidationOutcome::Blocked,
            &[(
                ValidatorClass::MissingRequiredSection,
                Some("Sources".into()),
            )],
            &[],
        );
        assert!(!decision.should_block);
        assert!(!decision.experimental);
        assert_eq!(decision.effective_outcome, ValidationOutcome::Warning);
        assert_eq!(decision.relaxed_classes.len(), 1);
        assert_eq!(
            decision.relaxed_classes[0].class,
            ValidatorClass::MissingRequiredSection
        );
        assert_eq!(
            decision.relaxed_classes[0].detail.as_deref(),
            Some("Sources")
        );
    }

    #[test]
    fn decide_soft_under_yolo_becomes_experimental() {
        let decision = decide_profile_validation(
            ExecutionProfile::Yolo,
            ValidationOutcome::Blocked,
            &[(ValidatorClass::MissingRequiredSection, None)],
            &[],
        );
        assert!(!decision.should_block);
        assert!(decision.experimental);
        assert_eq!(decision.effective_outcome, ValidationOutcome::Experimental);
    }

    #[test]
    fn decide_critical_blocks_in_yolo() {
        let decision = decide_profile_validation(
            ExecutionProfile::Yolo,
            ValidationOutcome::Blocked,
            &[
                (ValidatorClass::MissingRequiredSection, None),
                (ValidatorClass::DestructiveActionRequiresApproval, None),
            ],
            &[],
        );
        assert!(decision.should_block);
        assert_eq!(decision.effective_outcome, ValidationOutcome::Blocked);
        assert!(decision.relaxed_classes.is_empty());
    }

    #[test]
    fn decide_tenant_denylist_blocks_in_yolo() {
        let decision = decide_profile_validation(
            ExecutionProfile::Yolo,
            ValidationOutcome::Blocked,
            &[(ValidatorClass::MissingRequiredSection, None)],
            &[ValidatorClass::MissingRequiredSection],
        );
        assert!(decision.should_block);
        assert_eq!(decision.effective_outcome, ValidationOutcome::Blocked);
    }

    #[test]
    fn decide_yolo_only_class_not_relaxed_in_guided() {
        let decision = decide_profile_validation(
            ExecutionProfile::Guided,
            ValidationOutcome::Blocked,
            &[(ValidatorClass::MissingRequiredArtifactPath, None)],
            &[],
        );
        assert!(decision.should_block);
        assert_eq!(decision.effective_outcome, ValidationOutcome::Blocked);
    }

    #[test]
    fn repair_budget_multiplier_per_profile() {
        assert_eq!(effective_repair_budget(2, ExecutionProfile::Strict), 2);
        assert_eq!(effective_repair_budget(2, ExecutionProfile::Guided), 3);
        assert_eq!(effective_repair_budget(2, ExecutionProfile::Yolo), 4);
        assert_eq!(effective_repair_budget(0, ExecutionProfile::Yolo), 0);
        assert_eq!(effective_repair_budget(1, ExecutionProfile::Guided), 2);
    }
}
