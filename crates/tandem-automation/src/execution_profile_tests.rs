use crate::execution_profile::*;
use serde_json::{json, Value};

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
    assert_eq!(
        classify_unmet_requirement("required_source_paths_not_read"),
        Some(ValidatorClass::RequiredSourcePathsNotRead)
    );
    assert_eq!(
        classify_unmet_requirement("markdown_structure_missing"),
        Some(ValidatorClass::WeakMarkdownStructure)
    );
    assert_eq!(
        classify_unmet_requirement("editorial_substance_missing"),
        Some(ValidatorClass::MissingOptionalEvidence)
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
    let augmented =
        augment_output_with_profile_relaxation(&mut output, ExecutionProfile::Strict, None, &[]);
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
    let augmented =
        augment_output_with_profile_relaxation(&mut output, ExecutionProfile::Guided, None, &[]);
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
    let tainted = propagate_experimental_input_taint(&mut output, vec![("upstream", &upstream)]);
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
    let tainted = propagate_experimental_input_taint(&mut output, vec![("upstream", &upstream)]);
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

fn output_with_relaxed_classes(classes: &[&str], disposition: Option<&str>) -> Value {
    let entries: Vec<Value> = classes
        .iter()
        .map(|name| {
            json!({
                "class": name,
                "original_outcome": "blocked",
                "effective_outcome": "warning",
            })
        })
        .collect();
    let mut validation = json!({ "relaxed_validator_classes": entries });
    if let Some(value) = disposition {
        validation
            .as_object_mut()
            .unwrap()
            .insert("human_disposition".to_string(), json!(value));
    }
    json!({ "artifact_validation": validation })
}

#[test]
fn aggregate_dispositions_skips_outputs_without_relaxation() {
    let plain = json!({ "status": "completed" });
    let no_classes = json!({
        "artifact_validation": { "relaxed_validator_classes": [] }
    });
    let summary = aggregate_human_dispositions_by_class([&plain, &no_classes]);
    assert_eq!(summary.total_outputs_scanned, 2);
    assert_eq!(summary.total_relaxed_outputs, 0);
    assert!(summary.by_class.is_empty());
}

#[test]
fn aggregate_dispositions_attributes_to_every_relaxed_class() {
    let output = output_with_relaxed_classes(
        &["missing_required_section", "weak_markdown_structure"],
        Some("accepted"),
    );
    let summary = aggregate_human_dispositions_by_class([&output]);
    assert_eq!(summary.total_relaxed_outputs, 1);
    let mrs = summary
        .by_class
        .get(&ValidatorClass::MissingRequiredSection)
        .unwrap();
    assert_eq!(mrs.accepted, 1);
    let wms = summary
        .by_class
        .get(&ValidatorClass::WeakMarkdownStructure)
        .unwrap();
    assert_eq!(wms.accepted, 1);
}

#[test]
fn aggregate_dispositions_defaults_unmarked_when_no_signal() {
    let output = output_with_relaxed_classes(&["missing_required_section"], None);
    let summary = aggregate_human_dispositions_by_class([&output]);
    let counts = summary
        .by_class
        .get(&ValidatorClass::MissingRequiredSection)
        .unwrap();
    assert_eq!(counts.unmarked, 1);
    assert_eq!(counts.accepted, 0);
    assert!(counts.accept_rate().is_none());
}

#[test]
fn aggregate_dispositions_mixed_signals_per_class() {
    let outputs = vec![
        output_with_relaxed_classes(&["missing_required_section"], Some("accepted")),
        output_with_relaxed_classes(&["missing_required_section"], Some("accepted")),
        output_with_relaxed_classes(&["missing_required_section"], Some("rejected")),
        output_with_relaxed_classes(&["missing_required_section"], None),
    ];
    let summary = aggregate_human_dispositions_by_class(outputs.iter());
    let counts = summary
        .by_class
        .get(&ValidatorClass::MissingRequiredSection)
        .unwrap();
    assert_eq!(counts.accepted, 2);
    assert_eq!(counts.rejected, 1);
    assert_eq!(counts.unmarked, 1);
    assert_eq!(counts.total(), 4);
    // accept_rate excludes unmarked: 2 / (2 + 1) = 0.666...
    let rate = counts.accept_rate().unwrap();
    assert!((rate - (2.0 / 3.0)).abs() < 1e-6);
}

#[test]
fn aggregate_dispositions_skips_unknown_class_names() {
    let output = json!({
        "artifact_validation": {
            "relaxed_validator_classes": [
                {"class": "missing_required_section"},
                {"class": "totally_made_up_class"}
            ]
        }
    });
    let summary = aggregate_human_dispositions_by_class([&output]);
    assert_eq!(summary.total_relaxed_outputs, 1);
    assert_eq!(summary.by_class.len(), 1);
    assert!(summary
        .by_class
        .contains_key(&ValidatorClass::MissingRequiredSection));
}
