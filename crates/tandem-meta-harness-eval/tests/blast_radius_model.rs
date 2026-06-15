use std::collections::HashSet;

use tandem_meta_harness_eval::blast_radius::{
    mh04_scenarios, ActorRole, AttackClass, BlastRadiusScenario, ExposureDisposition,
    ExposureInvariant, RequestedOperationKind,
};

#[test]
fn mh04_blast_radius_scenarios_round_trip_through_stable_json() {
    let scenarios = mh04_scenarios();

    let json = serde_json::to_string_pretty(&scenarios).expect("serialize MH-04 scenarios");
    assert!(json.contains("mh04.prompt_injected_kb_mcp_bulk_export"));
    assert!(json.contains("prompt_injected_kb_mcp_bot"));
    assert!(json.contains("no_bulk_export"));
    assert!(json.contains("blocked"));
    assert!(json.contains("capped"));
    assert!(json.contains("redacted"));

    let decoded: Vec<BlastRadiusScenario> =
        serde_json::from_str(&json).expect("deserialize MH-04 scenarios");

    assert_eq!(decoded, scenarios);
}

#[test]
fn mh04_required_attack_classes_are_present() {
    let scenarios = mh04_scenarios();
    let ids: HashSet<_> = scenarios
        .iter()
        .map(|scenario| scenario.identity.id.as_str())
        .collect();

    assert_eq!(scenarios.len(), 3);
    assert_eq!(ids.len(), 3);
    assert!(ids.contains("mh04.prompt_injected_kb_mcp_bulk_export"));
    assert!(ids.contains("mh04.authorized_agent_broad_semantic_sweep"));
    assert!(ids.contains("mh04.poisoned_memory_authority_expansion"));

    assert!(scenarios.iter().any(|scenario| {
        scenario.identity.attack_class == AttackClass::PromptInjectedKbMcpBot
            && scenario.actor == ActorRole::KbMcpBot
            && scenario.requested_operation.kind == RequestedOperationKind::BulkMemoryExport
    }));
    assert!(scenarios.iter().any(|scenario| {
        scenario.identity.attack_class == AttackClass::BroadSemanticSweep
            && scenario.actor == ActorRole::AuthorizedAgent
            && scenario.requested_operation.kind == RequestedOperationKind::BroadSemanticSweep
    }));
    assert!(scenarios.iter().any(|scenario| {
        scenario.identity.attack_class == AttackClass::PoisonedMemoryAuthorityExpansion
            && scenario.actor == ActorRole::PoisonedMemoryEntry
            && scenario.requested_operation.kind == RequestedOperationKind::AuthorityWideningInstruction
    }));
}

#[test]
fn mh04_scenarios_encode_bounded_exposure_and_no_bulk_export() {
    for scenario in mh04_scenarios() {
        assert!(
            scenario.has_no_bulk_export_invariant(),
            "{} must deny bulk export",
            scenario.identity.id
        );
        assert!(
            !scenario.allowed_scope.may_export_bulk,
            "{} must not permit bulk export in its allowed scope",
            scenario.identity.id
        );
        assert!(
            scenario.expects_bounded_exposure(),
            "{} must cap exposure to its allowed scope",
            scenario.identity.id
        );
        assert!(
            matches!(
                scenario.expected_outcome.disposition,
                ExposureDisposition::Blocked
                    | ExposureDisposition::Capped
                    | ExposureDisposition::Redacted
                    | ExposureDisposition::Audited
            ),
            "{} must have an explicit bounded disposition",
            scenario.identity.id
        );
        assert!(
            scenario
                .invariants
                .contains(&ExposureInvariant::NoBulkExport),
            "{} must encode the no-bulk-export invariant",
            scenario.identity.id
        );
        assert!(
            scenario.audit_expectations.require_audit_event
                && scenario.audit_expectations.require_actor
                && scenario.audit_expectations.require_attack_vector
                && scenario.audit_expectations.require_scope_decision
                && scenario.audit_expectations.require_denied_bulk_export,
            "{} must require audit evidence for the containment decision",
            scenario.identity.id
        );
        assert!(
            scenario.expected_outcome.max_records_exposed <= scenario.allowed_scope.max_records,
            "{} exposes more records than its allowed scope",
            scenario.identity.id
        );
    }
}
