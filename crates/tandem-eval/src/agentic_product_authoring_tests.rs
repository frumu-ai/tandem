use super::*;

fn corpus_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../eval_datasets/agentic_product_authoring.yaml")
}

#[test]
fn corpus_is_versioned_and_has_all_required_scenario_kinds() {
    let document = load_agentic_dataset(&corpus_path()).expect("load authoring corpus");
    assert_eq!(document.dataset.name, "agentic_product_authoring");
    assert_eq!(document.acceptance.required_pass_rate, 1.0);
    assert!(!document.acceptance.live_provider_calls);
    let kinds = document
        .dataset
        .test_cases
        .iter()
        .map(|case| scenario_for_case(case).expect("scenario").name())
        .collect::<BTreeSet<_>>();
    for required in [
        "route",
        "chat_model_execution",
        "tool_contract",
        "identity_boundary",
        "draft_lifecycle",
        "active_artifact",
        "confirmation_boundary",
        "failure_taxonomy",
    ] {
        assert!(kinds.contains(required), "missing scenario kind {required}");
    }
}

#[test]
fn authoritative_success_claim_requires_a_persisted_artifact() {
    let outcome = json!({
        "ok": true,
        "resource": { "id": "automation-1" }
    });
    let error = validate_authoritative_claim(
        "Created the automation draft.",
        "automation-1",
        &outcome,
        None,
    )
    .expect_err("claim without persistence must fail");
    assert!(error.to_string().contains("persisted artifact"));
}

#[test]
fn missing_required_coverage_fails_the_gate() {
    let dataset = EvalDataset::new("coverage", "1.0");
    let thresholds = AgenticAuthoringAcceptanceThresholds {
        required_pass_rate: 1.0,
        required_coverage: vec!["tenant_isolation".to_string()],
        execution_profile: "deterministic".to_string(),
        live_provider_calls: false,
    };
    let report = build_report(
        &dataset,
        &thresholds,
        vec![AgenticAuthoringCaseResult {
            test_id: "case".to_string(),
            description: "case".to_string(),
            scenario: "route".to_string(),
            passed: true,
            tags: vec!["routing".to_string()],
            evidence: Value::Null,
            error: None,
        }],
    );
    assert!(!report.gate_passed);
    assert_eq!(report.missing_coverage, vec!["tenant_isolation"]);
}
