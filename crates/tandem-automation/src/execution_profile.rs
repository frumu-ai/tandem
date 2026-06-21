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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorClass {
    MissingRequiredSection,
    WeakMarkdownStructure,
    MissingOptionalEvidence,
    ArtifactWordCountBelowMinimum,
    MissingNonconsumedWorkspaceFiles,
    RequiredSourcePathsNotRead,
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
            ValidatorClass::RequiredSourcePathsNotRead => "required_source_paths_not_read",
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
/// default of Guided, so the precedence chain is:
///   run override → workflow policy → tenant default → Guided.
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
/// Guided/Lenient. Empty/unset returns an empty Vec — no classes are
/// denied beyond the always-critical hard set.
pub fn tenant_relaxation_denylist_from_env() -> Vec<ValidatorClass> {
    std::env::var("TANDEM_RELAXATION_DENYLIST")
        .ok()
        .as_deref()
        .map(parse_validator_class_list)
        .unwrap_or_default()
}

/// Human-applied accept/reject signal on a relaxed (Guided/Lenient) artifact.
///
/// Together with `relaxed_validator_classes`, this is the input to the
/// graduation loop: classes whose accept-rate is high enough over a rolling
/// window can be promoted from "experimental" to "supported", or moved
/// from Lenient into Guided. `Unmarked` is the default — it represents
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

/// Per-class accept/reject counts for graduation telemetry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DispositionCounts {
    #[serde(default)]
    pub accepted: u64,
    #[serde(default)]
    pub rejected: u64,
    #[serde(default)]
    pub re_ran_strict: u64,
    #[serde(default)]
    pub unmarked: u64,
}

impl DispositionCounts {
    pub fn record(&mut self, disposition: HumanDisposition) {
        match disposition {
            HumanDisposition::Accepted => self.accepted = self.accepted.saturating_add(1),
            HumanDisposition::Rejected => self.rejected = self.rejected.saturating_add(1),
            HumanDisposition::ReRanStrict => {
                self.re_ran_strict = self.re_ran_strict.saturating_add(1)
            }
            HumanDisposition::Unmarked => self.unmarked = self.unmarked.saturating_add(1),
        }
    }

    pub fn total(&self) -> u64 {
        self.accepted
            .saturating_add(self.rejected)
            .saturating_add(self.re_ran_strict)
            .saturating_add(self.unmarked)
    }

    /// Accept rate over reviewed dispositions (excludes `unmarked`). Returns
    /// `None` when no humans have reviewed any outputs in the bucket — the
    /// dashboard should render that as "insufficient signal" rather than 0%.
    pub fn accept_rate(&self) -> Option<f32> {
        let reviewed = self
            .accepted
            .saturating_add(self.rejected)
            .saturating_add(self.re_ran_strict);
        if reviewed == 0 {
            return None;
        }
        Some(self.accepted as f32 / reviewed as f32)
    }
}

/// Aggregate result of walking a slice of run records: per-`ValidatorClass`
/// disposition counts plus a few totals. Intended for the read-only
/// graduation summary endpoint and any future per-class graduation
/// dashboard. Pure — does not touch state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatorClassDispositionSummary {
    #[serde(default)]
    pub total_outputs_scanned: u64,
    #[serde(default)]
    pub total_relaxed_outputs: u64,
    #[serde(default)]
    pub by_class: std::collections::BTreeMap<ValidatorClass, DispositionCounts>,
}

/// Walk a slice of node outputs and attribute each output's
/// `human_disposition` (defaulting to `unmarked`) to **every** validator
/// class listed under `relaxed_validator_classes` for that output. Outputs
/// without `relaxed_validator_classes` are not included — they were not
/// relaxed under a profile and therefore have nothing to graduate.
///
/// Pure — does not touch state. The HTTP handler that surfaces this
/// aggregate is responsible for filtering runs by time window and
/// flattening the per-run `node_outputs` into the iterator.
pub fn aggregate_human_dispositions_by_class<'a, I>(outputs: I) -> ValidatorClassDispositionSummary
where
    I: IntoIterator<Item = &'a Value>,
{
    let mut summary = ValidatorClassDispositionSummary::default();
    for output in outputs {
        summary.total_outputs_scanned = summary.total_outputs_scanned.saturating_add(1);
        let validation = match output.get("artifact_validation") {
            Some(value) => value,
            None => continue,
        };
        let relaxed = match validation
            .get("relaxed_validator_classes")
            .and_then(Value::as_array)
        {
            Some(value) if !value.is_empty() => value,
            _ => continue,
        };
        summary.total_relaxed_outputs = summary.total_relaxed_outputs.saturating_add(1);
        let disposition = validation
            .get("human_disposition")
            .and_then(Value::as_str)
            .and_then(parse_human_disposition_str)
            .unwrap_or(HumanDisposition::Unmarked);
        for entry in relaxed {
            let class_name = entry
                .as_str()
                .or_else(|| entry.get("class").and_then(Value::as_str));
            let class_name = match class_name {
                Some(name) => name,
                None => continue,
            };
            if let Some(class) = parse_validator_class_list(class_name).into_iter().next() {
                summary
                    .by_class
                    .entry(class)
                    .or_default()
                    .record(disposition);
            }
        }
    }
    summary
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
        "weak_markdown_structure"
        | "weak_structure"
        | "weak_markdown"
        | "markdown_structure_missing" => Some(ValidatorClass::WeakMarkdownStructure),
        "missing_optional_evidence"
        | "missing_evidence_optional"
        | "editorial_substance_missing" => Some(ValidatorClass::MissingOptionalEvidence),
        "artifact_word_count_below_minimum" | "artifact_too_short" => {
            Some(ValidatorClass::ArtifactWordCountBelowMinimum)
        }
        "missing_nonconsumed_workspace_files" | "missing_optional_workspace_files" => {
            Some(ValidatorClass::MissingNonconsumedWorkspaceFiles)
        }
        "required_source_paths_not_read" | "required_source_read_paths_not_read" => {
            Some(ValidatorClass::RequiredSourcePathsNotRead)
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
///   `completed` (Lenient; experimental-flagged via `artifact_validation`).
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
                    .split_once([':', '|'])
                    .map(|(_, tail)| tail)
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
