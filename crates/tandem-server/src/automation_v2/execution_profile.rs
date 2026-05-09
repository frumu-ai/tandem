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

/// Marks an output as carrying experimental input taint when one or more
/// upstream node outputs are themselves experimental. Pure metadata: writes
/// `artifact_validation.experimental = true` and
/// `artifact_validation.tainted_inputs = [upstream_node_id, ...]` without
/// touching `output.status`. Returns `true` when taint was applied.
///
/// Rationale (PROPOSAL.md "Experimental Propagation"): a downstream node's
/// own validation may pass even when its inputs were accepted under a
/// relaxed profile. Without taint propagation the run-level
/// "experimental" flag would silently disappear at the first cleanly-passing
/// downstream step. Propagating taint keeps receipts honest and lets
/// `run_completed` consumers filter experimental runs.
pub fn propagate_experimental_input_taint<'a, I>(output: &mut Value, upstream_outputs: I) -> bool
where
    I: IntoIterator<Item = (&'a str, &'a Value)>,
{
    let tainted: Vec<String> = upstream_outputs
        .into_iter()
        .filter_map(|(node_id, upstream_output)| {
            let is_experimental = upstream_output
                .get("artifact_validation")
                .and_then(|av| av.get("experimental"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_experimental {
                Some(node_id.to_string())
            } else {
                None
            }
        })
        .collect();
    if tainted.is_empty() {
        return false;
    }

    let object = match output.as_object_mut() {
        Some(map) => map,
        None => return false,
    };
    if !object.contains_key("artifact_validation") {
        object.insert(
            "artifact_validation".to_string(),
            Value::Object(serde_json::Map::new()),
        );
    }
    let validation = object
        .get_mut("artifact_validation")
        .and_then(Value::as_object_mut)
        .expect("artifact_validation present (just inserted if missing)");

    let already_experimental = validation
        .get("experimental")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    validation.insert("experimental".to_string(), json!(true));
    validation
        .entry("tainted_inputs".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(arr) = validation
        .get_mut("tainted_inputs")
        .and_then(Value::as_array_mut)
    {
        for node_id in tainted {
            let already_listed = arr.iter().any(|value| value.as_str() == Some(&node_id));
            if !already_listed {
                arr.push(json!(node_id));
            }
        }
    }
    !already_experimental
}

/// Parses a string into an `ExecutionProfile`, accepting the same
/// snake_case wire form as serde plus a few common aliases. Trims and
/// lowercases the input. Empty strings and unknown values return `None`.
///
/// Used for parsing operator-supplied tenant-default settings (e.g.
/// `TANDEM_DEFAULT_EXECUTION_PROFILE` env var) without forcing operators
/// to remember exact casing.
pub fn parse_execution_profile_str(raw: &str) -> Option<ExecutionProfile> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "strict" => Some(ExecutionProfile::Strict),
        "guided" | "assisted" | "warn" => Some(ExecutionProfile::Guided),
        "yolo" | "exploratory" | "lenient" | "permissive" => Some(ExecutionProfile::Yolo),
        _ => None,
    }
}

/// Reads the tenant-level default execution profile from the
/// `TANDEM_DEFAULT_EXECUTION_PROFILE` environment variable. Returns
/// `None` when the variable is unset, empty, or names an unknown value
/// (operators get safe Strict fallback rather than a panic on typos).
///
/// Run-creation paths consult this before falling back to the system
/// default of Strict, so the precedence chain is:
///   run override → workflow policy → tenant default → Strict.
pub fn tenant_default_execution_profile_from_env() -> Option<ExecutionProfile> {
    std::env::var("TANDEM_DEFAULT_EXECUTION_PROFILE")
        .ok()
        .as_deref()
        .and_then(parse_execution_profile_str)
}

/// Parses a comma-separated list of validator class names into the
/// `ValidatorClass` taxonomy. Trims and lowercases each entry; unknown
/// entries are silently skipped (operators get a safe under-restriction
/// fallback rather than a panic on typos). Recognized inputs match the
/// canonical `as_str` form, e.g. `missing_required_section`,
/// `weak_markdown_structure`, `repair_budget_exhausted`.
pub fn parse_validator_class_list(raw: &str) -> Vec<ValidatorClass> {
    raw.split(',')
        .filter_map(|item| {
            let normalized = item.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "missing_required_section" => Some(ValidatorClass::MissingRequiredSection),
                "weak_markdown_structure" => Some(ValidatorClass::WeakMarkdownStructure),
                "missing_optional_evidence" => Some(ValidatorClass::MissingOptionalEvidence),
                "artifact_word_count_below_minimum" => {
                    Some(ValidatorClass::ArtifactWordCountBelowMinimum)
                }
                "missing_nonconsumed_workspace_files" => {
                    Some(ValidatorClass::MissingNonconsumedWorkspaceFiles)
                }
                "missing_required_artifact_path" => {
                    Some(ValidatorClass::MissingRequiredArtifactPath)
                }
                "validator_kind_specific_soft_check" => {
                    Some(ValidatorClass::ValidatorKindSpecificSoftCheck)
                }
                "repair_budget_exhausted" => Some(ValidatorClass::RepairBudgetExhausted),
                _ => None,
            }
        })
        .collect()
}

/// Reads the tenant-level relaxation denylist from the
/// `TANDEM_RELAXATION_DENYLIST` environment variable. Returns the list
/// of `ValidatorClass` values that should NEVER be relaxed under any
/// profile, even when the chokepoint would otherwise allow them.
///
/// Operators set this to insist that specific validator classes always
/// block (e.g. `missing_required_artifact_path,repair_budget_exhausted`)
/// while still benefiting from the rest of the relaxation set under
/// Guided/YOLO. Empty/unset returns an empty Vec — no classes are
/// denied beyond the always-critical hard set.
pub fn tenant_relaxation_denylist_from_env() -> Vec<ValidatorClass> {
    std::env::var("TANDEM_RELAXATION_DENYLIST")
        .ok()
        .as_deref()
        .map(parse_validator_class_list)
        .unwrap_or_default()
}

/// Human-applied accept/reject signal on a relaxed (Guided/YOLO) artifact.
///
/// Together with `relaxed_validator_classes`, this is the input to the
/// graduation loop: classes whose accept-rate is high enough over a rolling
/// window can be promoted from "experimental" to "supported", or moved
/// from YOLO into Guided. `Unmarked` is the default — it represents
/// "no human has reviewed this yet" rather than a neutral verdict, and
/// must not be confused with `Accepted`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum HumanDisposition {
    #[default]
    Unmarked,
    Accepted,
    Rejected,
    ReRanStrict,
}

impl HumanDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            HumanDisposition::Unmarked => "unmarked",
            HumanDisposition::Accepted => "accepted",
            HumanDisposition::Rejected => "rejected",
            HumanDisposition::ReRanStrict => "re_ran_strict",
        }
    }
}

/// Parses a human-disposition string from API/UI input. Accepts the canonical
/// snake_case form plus a few operator-friendly aliases (`approve`/`reject`/
/// `rerun`). Whitespace and case are normalized. Unknown strings return
/// `None` — callers should reject those rather than silently coercing.
pub fn parse_human_disposition_str(raw: &str) -> Option<HumanDisposition> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "unmarked" | "" | "none" | "clear" => Some(HumanDisposition::Unmarked),
        "accepted" | "accept" | "approve" | "approved" | "ok" => Some(HumanDisposition::Accepted),
        "rejected" | "reject" | "deny" | "denied" | "fail" => Some(HumanDisposition::Rejected),
        "re_ran_strict" | "rerun_strict" | "rerun-strict" | "rerun" | "re_ran" => {
            Some(HumanDisposition::ReRanStrict)
        }
        _ => None,
    }
}

/// Writes `human_disposition` into `output["artifact_validation"]`. Returns
/// `true` when the value was newly set or changed; `false` when the key
/// already held the same disposition. Creates an empty `artifact_validation`
/// object if one is not yet present, so dispositions can be set on outputs
/// that did not go through the relaxation chokepoint (e.g. Strict runs the
/// human still wants to comment on).
pub fn set_human_disposition_on_output(output: &mut Value, disposition: HumanDisposition) -> bool {
    let object = match output.as_object_mut() {
        Some(map) => map,
        None => return false,
    };
    if !object.contains_key("artifact_validation") {
        object.insert(
            "artifact_validation".to_string(),
            Value::Object(serde_json::Map::new()),
        );
    }
    let validation = match object
        .get_mut("artifact_validation")
        .and_then(Value::as_object_mut)
    {
        Some(map) => map,
        None => return false,
    };
    let previous = validation
        .get("human_disposition")
        .and_then(Value::as_str)
        .map(str::to_string);
    let next = disposition.as_str().to_string();
    if previous.as_deref() == Some(next.as_str()) {
        return false;
    }
    validation.insert("human_disposition".to_string(), json!(next));
    true
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
    fn taint_propagation_marks_downstream_experimental() {
        let mut output = json!({
            "status": "completed",
            "artifact_validation": {
                "validation_outcome": "passed",
            }
        });
        let upstream_a = json!({
            "artifact_validation": { "experimental": true }
        });
        let upstream_b = json!({
            "artifact_validation": { "experimental": false }
        });
        let tainted = propagate_experimental_input_taint(
            &mut output,
            vec![("node-a", &upstream_a), ("node-b", &upstream_b)],
        );
        assert!(tainted);
        let validation = output.pointer("/artifact_validation").unwrap();
        assert_eq!(
            validation.get("experimental").and_then(Value::as_bool),
            Some(true)
        );
        let tainted_inputs = validation
            .get("tainted_inputs")
            .and_then(Value::as_array)
            .unwrap();
        let names: Vec<&str> = tainted_inputs.iter().filter_map(Value::as_str).collect();
        assert_eq!(names, vec!["node-a"]);
        // Status is intentionally unchanged by taint propagation.
        assert_eq!(
            output.get("status").and_then(Value::as_str),
            Some("completed")
        );
    }

    #[test]
    fn taint_propagation_no_op_when_no_upstream_experimental() {
        let mut output = json!({
            "artifact_validation": { "validation_outcome": "passed" }
        });
        let upstream = json!({ "artifact_validation": { "experimental": false } });
        let tainted = propagate_experimental_input_taint(&mut output, vec![("node-a", &upstream)]);
        assert!(!tainted);
        let validation = output.pointer("/artifact_validation").unwrap();
        assert!(validation.get("experimental").is_none());
        assert!(validation.get("tainted_inputs").is_none());
    }

    #[test]
    fn taint_propagation_creates_artifact_validation_when_absent() {
        let mut output = json!({ "status": "completed" });
        let upstream = json!({ "artifact_validation": { "experimental": true } });
        let tainted =
            propagate_experimental_input_taint(&mut output, vec![("upstream", &upstream)]);
        assert!(tainted);
        let validation = output.pointer("/artifact_validation").unwrap();
        assert_eq!(
            validation.get("experimental").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn taint_propagation_already_experimental_returns_false() {
        let mut output = json!({
            "artifact_validation": { "experimental": true }
        });
        let upstream = json!({ "artifact_validation": { "experimental": true } });
        let tainted =
            propagate_experimental_input_taint(&mut output, vec![("upstream", &upstream)]);
        // Returns false because it was already experimental, but tainted_inputs
        // should still be populated for receipts.
        assert!(!tainted);
        let validation = output.pointer("/artifact_validation").unwrap();
        let tainted_inputs = validation
            .get("tainted_inputs")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(tainted_inputs.len(), 1);
    }

    #[test]
    fn parse_execution_profile_accepts_canonical_and_aliases() {
        assert_eq!(
            parse_execution_profile_str("strict"),
            Some(ExecutionProfile::Strict)
        );
        assert_eq!(
            parse_execution_profile_str("Strict"),
            Some(ExecutionProfile::Strict)
        );
        assert_eq!(
            parse_execution_profile_str("  STRICT  "),
            Some(ExecutionProfile::Strict)
        );
        assert_eq!(
            parse_execution_profile_str("guided"),
            Some(ExecutionProfile::Guided)
        );
        assert_eq!(
            parse_execution_profile_str("assisted"),
            Some(ExecutionProfile::Guided)
        );
        assert_eq!(
            parse_execution_profile_str("yolo"),
            Some(ExecutionProfile::Yolo)
        );
        assert_eq!(
            parse_execution_profile_str("exploratory"),
            Some(ExecutionProfile::Yolo)
        );
        assert_eq!(
            parse_execution_profile_str("lenient"),
            Some(ExecutionProfile::Yolo)
        );
    }

    #[test]
    fn parse_execution_profile_rejects_unknown_strings() {
        assert_eq!(parse_execution_profile_str(""), None);
        assert_eq!(parse_execution_profile_str("loose"), None);
        assert_eq!(parse_execution_profile_str("relaxed"), None);
        assert_eq!(parse_execution_profile_str("danger"), None);
    }

    #[test]
    fn parse_validator_class_list_handles_canonical_names() {
        let parsed = parse_validator_class_list(
            "missing_required_section, weak_markdown_structure,repair_budget_exhausted",
        );
        assert_eq!(
            parsed,
            vec![
                ValidatorClass::MissingRequiredSection,
                ValidatorClass::WeakMarkdownStructure,
                ValidatorClass::RepairBudgetExhausted,
            ]
        );
    }

    #[test]
    fn parse_validator_class_list_skips_unknown_entries() {
        let parsed = parse_validator_class_list(
            "missing_required_section,not_a_real_class,weak_markdown_structure",
        );
        assert_eq!(
            parsed,
            vec![
                ValidatorClass::MissingRequiredSection,
                ValidatorClass::WeakMarkdownStructure,
            ]
        );
    }

    #[test]
    fn parse_validator_class_list_handles_empty_and_whitespace() {
        assert!(parse_validator_class_list("").is_empty());
        assert!(parse_validator_class_list("   ,, ,").is_empty());
        assert_eq!(
            parse_validator_class_list("  WEAK_MARKDOWN_STRUCTURE  "),
            vec![ValidatorClass::WeakMarkdownStructure]
        );
    }

    #[test]
    fn denylisted_class_blocks_under_yolo_via_decision() {
        let denylist = vec![ValidatorClass::MissingRequiredSection];
        let decision = decide_profile_validation(
            ExecutionProfile::Yolo,
            ValidationOutcome::Blocked,
            &[(ValidatorClass::MissingRequiredSection, None)],
            &denylist,
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

    #[test]
    fn parse_human_disposition_canonical_strings() {
        assert_eq!(
            parse_human_disposition_str("accepted"),
            Some(HumanDisposition::Accepted)
        );
        assert_eq!(
            parse_human_disposition_str("rejected"),
            Some(HumanDisposition::Rejected)
        );
        assert_eq!(
            parse_human_disposition_str("re_ran_strict"),
            Some(HumanDisposition::ReRanStrict)
        );
        assert_eq!(
            parse_human_disposition_str("unmarked"),
            Some(HumanDisposition::Unmarked)
        );
    }

    #[test]
    fn parse_human_disposition_aliases_and_normalization() {
        assert_eq!(
            parse_human_disposition_str("  ACCEPT  "),
            Some(HumanDisposition::Accepted)
        );
        assert_eq!(
            parse_human_disposition_str("Reject"),
            Some(HumanDisposition::Rejected)
        );
        assert_eq!(
            parse_human_disposition_str("rerun"),
            Some(HumanDisposition::ReRanStrict)
        );
        assert_eq!(
            parse_human_disposition_str(""),
            Some(HumanDisposition::Unmarked)
        );
        assert_eq!(parse_human_disposition_str("maybe"), None);
    }

    #[test]
    fn set_human_disposition_writes_into_artifact_validation() {
        let mut output = json!({
            "status": "completed_with_warnings",
            "artifact_validation": {
                "execution_profile": "guided",
                "relaxed_validator_classes": [{"class": "missing_required_section"}],
            },
        });
        let changed = set_human_disposition_on_output(&mut output, HumanDisposition::Accepted);
        assert!(changed);
        assert_eq!(
            output
                .pointer("/artifact_validation/human_disposition")
                .and_then(Value::as_str),
            Some("accepted")
        );
    }

    #[test]
    fn set_human_disposition_creates_validation_object_when_absent() {
        let mut output = json!({ "status": "completed" });
        let changed = set_human_disposition_on_output(&mut output, HumanDisposition::ReRanStrict);
        assert!(changed);
        assert_eq!(
            output
                .pointer("/artifact_validation/human_disposition")
                .and_then(Value::as_str),
            Some("re_ran_strict")
        );
    }

    #[test]
    fn set_human_disposition_is_idempotent_on_same_value() {
        let mut output = json!({
            "artifact_validation": { "human_disposition": "accepted" }
        });
        let changed = set_human_disposition_on_output(&mut output, HumanDisposition::Accepted);
        assert!(!changed);
    }

    #[test]
    fn set_human_disposition_overwrites_previous_value() {
        let mut output = json!({
            "artifact_validation": { "human_disposition": "accepted" }
        });
        let changed = set_human_disposition_on_output(&mut output, HumanDisposition::Rejected);
        assert!(changed);
        assert_eq!(
            output
                .pointer("/artifact_validation/human_disposition")
                .and_then(Value::as_str),
            Some("rejected")
        );
    }
}
