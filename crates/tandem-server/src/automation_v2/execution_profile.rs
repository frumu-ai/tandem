use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

/// Classifies a validator's `unmet_requirements` string into a
/// `ValidatorClass`. Returns `None` for strings that have not yet been
/// taxonomized — those default to "blocking, never relaxable" so behavior
/// stays Strict-equivalent until the class is explicitly added.
pub fn classify_unmet_requirement(raw: &str) -> Option<ValidatorClass> {
    let key = raw
        .split([':', '|'])
        .next()
        .map(str::trim)
        .unwrap_or(raw)
        .trim();
    match key {
        "missing_required_section" | "missing_section" | "section_missing" => {
            Some(ValidatorClass::MissingRequiredSection)
        }
        "weak_markdown_structure" | "weak_structure" | "weak_markdown" => {
            Some(ValidatorClass::WeakMarkdownStructure)
        }
        "missing_optional_evidence" | "missing_evidence_optional" => {
            Some(ValidatorClass::MissingOptionalEvidence)
        }
        "artifact_word_count_below_minimum" | "artifact_too_short" => {
            Some(ValidatorClass::ArtifactWordCountBelowMinimum)
        }
        "missing_nonconsumed_workspace_files" | "missing_optional_workspace_files" => {
            Some(ValidatorClass::MissingNonconsumedWorkspaceFiles)
        }
        "missing_required_artifact_path" | "missing_artifact_path" => {
            Some(ValidatorClass::MissingRequiredArtifactPath)
        }
        "validator_kind_specific_soft_check" | "soft_validator_check" => {
            Some(ValidatorClass::ValidatorKindSpecificSoftCheck)
        }
        "repair_budget_exhausted" => Some(ValidatorClass::RepairBudgetExhausted),
        "unauthorized_workspace" | "workspace_unauthorized" => {
            Some(ValidatorClass::UnauthorizedWorkspace)
        }
        "secret_access_denied" => Some(ValidatorClass::SecretAccessDenied),
        "destructive_action_requires_approval" | "destructive_requires_approval" => {
            Some(ValidatorClass::DestructiveActionRequiresApproval)
        }
        "external_publish_requires_approval" => {
            Some(ValidatorClass::ExternalPublishRequiresApproval)
        }
        "tenant_policy_denied" | "policy_denied" => Some(ValidatorClass::TenantPolicyDenied),
        "tool_unauthorized" | "unauthorized_tool" => Some(ValidatorClass::ToolUnauthorized),
        "budget_exceeded" => Some(ValidatorClass::BudgetExceeded),
        "kill_switch_engaged" => Some(ValidatorClass::KillSwitchEngaged),
        "engine_lease_expired" => Some(ValidatorClass::EngineLeaseExpired),
        "invalid_api_token" => Some(ValidatorClass::InvalidApiToken),
        "deterministic_verification_failed" | "code_patch_apply_failed" => {
            Some(ValidatorClass::DeterministicVerificationFailed)
        }
        _ => None,
    }
}

/// Augments a node `output` JSON value with profile-aware relaxation
/// metadata AND rewrites the executor's blocking signals when the active
/// profile would relax all of its unmet requirements.
///
/// On a successful relaxation, this function writes telemetry into
/// `output["artifact_validation"]` (`relaxed_validator_classes`,
/// `effective_outcome`, `original_validator_outcome`, `execution_profile`,
/// optional `requested_execution_profile`, `experimental`, and
/// `original_status`) AND downgrades the executor-facing fields so the run
/// continues:
///
/// - `output["status"]` becomes `completed_with_warnings` (Guided) or
///   `completed` (YOLO; experimental-flagged via `artifact_validation`).
/// - `output["failure_kind"]` is cleared if it was validation-related.
/// - `output["blocked_reason"]` is cleared.
/// - `artifact_validation.warning_count` is set to the count of relaxed
///   classes so `automation_output_has_warnings` returns true.
///
/// Strict runs and runs whose unmet requirements include any critical or
/// not-yet-classified class are returned untouched.
///
/// Returns `true` when relaxation occurred.
pub fn augment_output_with_profile_relaxation(
    output: &mut Value,
    profile: ExecutionProfile,
    requested_profile: Option<ExecutionProfile>,
    tenant_relaxation_denylist: &[ValidatorClass],
) -> bool {
    let object = match output.as_object_mut() {
        Some(map) => map,
        None => return false,
    };
    let raw_unmet = object
        .get("artifact_validation")
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if raw_unmet.is_empty() {
        return false;
    }
    let mut classes: Vec<(ValidatorClass, Option<String>)> = Vec::new();
    let mut had_unclassified = false;
    for entry in &raw_unmet {
        let raw = match entry.as_str() {
            Some(value) => value.trim(),
            None => continue,
        };
        match classify_unmet_requirement(raw) {
            Some(class) => {
                let detail = raw
                    .splitn(2, [':', '|'])
                    .nth(1)
                    .map(|tail| tail.trim().to_string())
                    .filter(|value| !value.is_empty());
                classes.push((class, detail));
            }
            None => {
                had_unclassified = true;
            }
        }
    }

    let original_outcome = ValidationOutcome::Blocked;
    let decision = decide_profile_validation(
        profile,
        original_outcome,
        &classes,
        tenant_relaxation_denylist,
    );
    let augmented = !decision.relaxed_classes.is_empty()
        && !matches!(decision.effective_outcome, ValidationOutcome::Blocked);
    if !augmented {
        return false;
    }
    if had_unclassified {
        // Conservative: if any unmet requirement is not yet classified, keep
        // Strict-equivalent behavior even when others would relax.
        return false;
    }

    let original_status = object
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let original_failure_kind = object
        .get("failure_kind")
        .and_then(Value::as_str)
        .map(str::to_string);

    // Downgrade executor-facing blocking signals so the run continues.
    let new_status = match decision.effective_outcome {
        ValidationOutcome::Warning => "completed_with_warnings",
        ValidationOutcome::Experimental => "completed",
        // Defensive: by construction `effective_outcome` is non-blocking here.
        ValidationOutcome::Passed | ValidationOutcome::Blocked => "completed",
    };
    object.insert("status".to_string(), json!(new_status));
    let validation_failure_kinds = matches!(
        original_failure_kind.as_deref(),
        Some("validation_error") | Some("verification_failed") | Some("artifact_rejected")
    );
    if validation_failure_kinds {
        object.insert("failure_kind".to_string(), Value::Null);
    }
    if matches!(
        object.get("blocked_reason").and_then(Value::as_str),
        Some(text) if !text.is_empty()
    ) {
        object.insert("blocked_reason".to_string(), Value::Null);
    }

    let validation = object
        .get_mut("artifact_validation")
        .and_then(Value::as_object_mut)
        .expect("artifact_validation present (checked above)");
    validation.insert(
        "relaxed_validator_classes".to_string(),
        serde_json::to_value(&decision.relaxed_classes).unwrap_or(Value::Null),
    );
    validation.insert(
        "effective_outcome".to_string(),
        json!(decision.effective_outcome.as_str()),
    );
    validation.insert(
        "original_validator_outcome".to_string(),
        json!(original_outcome.as_str()),
    );
    validation.insert("execution_profile".to_string(), json!(profile.as_str()));
    if let Some(req) = requested_profile {
        validation.insert(
            "requested_execution_profile".to_string(),
            json!(req.as_str()),
        );
    }
    if decision.experimental {
        validation.insert("experimental".to_string(), json!(true));
    }
    if let Some(prev) = original_status {
        validation.insert("original_status".to_string(), json!(prev));
    }
    if let Some(prev) = original_failure_kind {
        validation.insert("original_failure_kind".to_string(), json!(prev));
    }
    let warning_count = decision.relaxed_classes.len() as u64;
    validation.insert("warning_count".to_string(), json!(warning_count));
    true
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
    fn classify_known_strings_to_validator_classes() {
        assert_eq!(
            classify_unmet_requirement("missing_required_section"),
            Some(ValidatorClass::MissingRequiredSection)
        );
        assert_eq!(
            classify_unmet_requirement("missing_required_section: Sources"),
            Some(ValidatorClass::MissingRequiredSection)
        );
        assert_eq!(
            classify_unmet_requirement("destructive_action_requires_approval"),
            Some(ValidatorClass::DestructiveActionRequiresApproval)
        );
        assert_eq!(
            classify_unmet_requirement("budget_exceeded"),
            Some(ValidatorClass::BudgetExceeded)
        );
        assert_eq!(classify_unmet_requirement("totally_unknown_class"), None);
    }

    #[test]
    fn classifier_critical_strings_remain_critical() {
        let critical_strings = [
            "unauthorized_workspace",
            "secret_access_denied",
            "destructive_action_requires_approval",
            "tenant_policy_denied",
            "tool_unauthorized",
            "budget_exceeded",
            "kill_switch_engaged",
            "deterministic_verification_failed",
        ];
        for raw in critical_strings {
            let class = classify_unmet_requirement(raw)
                .unwrap_or_else(|| panic!("expected classification for {raw}"));
            assert!(
                class.is_critical(),
                "{raw} -> {:?} should be critical",
                class
            );
        }
    }

    #[test]
    fn augment_strict_profile_no_change() {
        let mut output = json!({
            "status": "verify_failed",
            "failure_kind": "validation_error",
            "artifact_validation": {
                "unmet_requirements": ["missing_required_section: Sources"],
            }
        });
        let augmented = augment_output_with_profile_relaxation(
            &mut output,
            ExecutionProfile::Strict,
            None,
            &[],
        );
        assert!(!augmented);
        // Status and failure_kind are preserved under Strict.
        assert_eq!(
            output.get("status").and_then(Value::as_str),
            Some("verify_failed")
        );
        assert_eq!(
            output.get("failure_kind").and_then(Value::as_str),
            Some("validation_error")
        );
        let validation = output.pointer("/artifact_validation").unwrap();
        assert!(validation.get("relaxed_validator_classes").is_none());
        assert!(validation.get("effective_outcome").is_none());
        assert!(validation.get("warning_count").is_none());
    }

    #[test]
    fn augment_guided_writes_warning_outcome_and_downgrades_status() {
        let mut output = json!({
            "status": "verify_failed",
            "failure_kind": "validation_error",
            "blocked_reason": "missing required section `Sources`",
            "artifact_validation": {
                "unmet_requirements": ["missing_required_section: Sources"],
            }
        });
        let augmented = augment_output_with_profile_relaxation(
            &mut output,
            ExecutionProfile::Guided,
            None,
            &[],
        );
        assert!(augmented);
        assert_eq!(
            output.get("status").and_then(Value::as_str),
            Some("completed_with_warnings")
        );
        assert!(output.get("failure_kind").map_or(true, Value::is_null));
        assert!(output.get("blocked_reason").map_or(true, Value::is_null));
        let validation = output.pointer("/artifact_validation").unwrap();
        assert_eq!(
            validation.get("effective_outcome").and_then(Value::as_str),
            Some("warning")
        );
        assert_eq!(
            validation
                .get("original_validator_outcome")
                .and_then(Value::as_str),
            Some("blocked")
        );
        assert_eq!(
            validation.get("execution_profile").and_then(Value::as_str),
            Some("guided")
        );
        assert_eq!(
            validation.get("original_status").and_then(Value::as_str),
            Some("verify_failed")
        );
        assert_eq!(
            validation
                .get("original_failure_kind")
                .and_then(Value::as_str),
            Some("validation_error")
        );
        assert_eq!(
            validation.get("warning_count").and_then(Value::as_u64),
            Some(1)
        );
        assert!(validation.get("experimental").is_none());
        let classes = validation
            .get("relaxed_validator_classes")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(classes.len(), 1);
        assert_eq!(
            classes[0].get("class").and_then(Value::as_str),
            Some("missing_required_section")
        );
        assert_eq!(
            classes[0].get("detail").and_then(Value::as_str),
            Some("Sources")
        );
    }

    #[test]
    fn augment_yolo_writes_experimental_flag_and_completes_node() {
        let mut output = json!({
            "status": "verify_failed",
            "failure_kind": "validation_error",
            "artifact_validation": {
                "unmet_requirements": ["missing_required_section: Sources"],
            }
        });
        let augmented = augment_output_with_profile_relaxation(
            &mut output,
            ExecutionProfile::Yolo,
            Some(ExecutionProfile::Yolo),
            &[],
        );
        assert!(augmented);
        assert_eq!(
            output.get("status").and_then(Value::as_str),
            Some("completed")
        );
        assert!(output.get("failure_kind").map_or(true, Value::is_null));
        let validation = output.pointer("/artifact_validation").unwrap();
        assert_eq!(
            validation.get("effective_outcome").and_then(Value::as_str),
            Some("experimental")
        );
        assert_eq!(
            validation.get("experimental").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            validation
                .get("requested_execution_profile")
                .and_then(Value::as_str),
            Some("yolo")
        );
        assert_eq!(
            validation.get("warning_count").and_then(Value::as_u64),
            Some(1)
        );
    }

    #[test]
    fn augment_yolo_preserves_non_validation_failure_kind() {
        // If the failure_kind is something we did not originate
        // (e.g. provider stream error), do not clear it even when relaxing.
        let mut output = json!({
            "status": "verify_failed",
            "failure_kind": "provider_stream_failed",
            "artifact_validation": {
                "unmet_requirements": ["missing_required_section: Sources"],
            }
        });
        augment_output_with_profile_relaxation(&mut output, ExecutionProfile::Yolo, None, &[]);
        assert_eq!(
            output.get("failure_kind").and_then(Value::as_str),
            Some("provider_stream_failed")
        );
    }

    #[test]
    fn augment_critical_class_blocks_under_yolo() {
        let mut output = json!({
            "artifact_validation": {
                "unmet_requirements": [
                    "missing_required_section: Sources",
                    "destructive_action_requires_approval"
                ],
            }
        });
        let augmented =
            augment_output_with_profile_relaxation(&mut output, ExecutionProfile::Yolo, None, &[]);
        assert!(!augmented);
    }

    #[test]
    fn augment_unclassified_string_is_conservative() {
        let mut output = json!({
            "artifact_validation": {
                "unmet_requirements": [
                    "missing_required_section: Sources",
                    "totally_unknown_class"
                ],
            }
        });
        let augmented =
            augment_output_with_profile_relaxation(&mut output, ExecutionProfile::Yolo, None, &[]);
        assert!(!augmented);
    }

    #[test]
    fn augment_no_unmet_requirements_no_change() {
        let mut output = json!({
            "artifact_validation": {
                "unmet_requirements": [],
            }
        });
        let augmented =
            augment_output_with_profile_relaxation(&mut output, ExecutionProfile::Yolo, None, &[]);
        assert!(!augmented);
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
