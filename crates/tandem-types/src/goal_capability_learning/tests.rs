use super::*;

#[test]
fn demo_csv_goal_structure_is_valid() {
    let goal = GoalSpec {
        goal_id: "demo_csv_parse".to_string(),
        title: "Read and parse CSV file".to_string(),
        description: "Parse CSV file into objects".to_string(),
        input_parameters: vec![GoalParameter {
            name: "file_path".to_string(),
            data_type: "String".to_string(),
            description: "Path to CSV file".to_string(),
            required: true,
        }],
        expected_output_format: "Array of objects (JSON)".to_string(),
        constraints: vec![],
    };

    assert_eq!(goal.goal_id, "demo_csv_parse");
    assert!(!goal.input_parameters.is_empty());
}

#[test]
fn capability_discovery_report_primary_recommendation() {
    let report = CapabilityDiscoveryReport {
        goal_id: "demo_csv_parse".to_string(),
        requirements: vec![],
        discovered_capabilities: vec![],
        composition_candidates: vec![
            CompositionPath {
                sequence: vec!["file_read".to_string(), "csv_parse".to_string()],
                compatibility_score: 0.9,
                reasoning: "Standard file I/O + parsing".to_string(),
            },
            CompositionPath {
                sequence: vec!["file_read".to_string(), "json_decode".to_string()],
                compatibility_score: 0.3,
                reasoning: "Wrong format".to_string(),
            },
        ],
        gaps: vec![],
        overall_confidence_score: 0.9,
        reasoning: "Found file_read and csv_parse".to_string(),
    };

    let primary = report.primary_recommendation();
    assert!(primary.is_some());
    assert_eq!(primary.unwrap().compatibility_score, 0.9);
}

#[test]
fn goal_spec_round_trips_json() {
    let goal = GoalSpec {
        goal_id: "test".to_string(),
        title: "Test".to_string(),
        description: "Test goal".to_string(),
        input_parameters: vec![],
        expected_output_format: "JSON".to_string(),
        constraints: vec![],
    };

    let encoded = serde_json::to_value(&goal).expect("serialize");
    let decoded: GoalSpec = serde_json::from_value(encoded).expect("deserialize");
    assert_eq!(decoded, goal);
}

#[test]
fn capability_gap_variants_serialize() {
    let gaps = vec![
        CapabilityGap::NotFound {
            description: "No CSV parser".to_string(),
        },
        CapabilityGap::NotAuthorized {
            capability_id: "secure_file_read".to_string(),
        },
        CapabilityGap::RejectedByConstraint {
            capability_id: "external_api".to_string(),
            reason: "No external APIs".to_string(),
        },
    ];

    for gap in gaps {
        let encoded = serde_json::to_value(&gap).expect("serialize gap");
        let decoded: CapabilityGap = serde_json::from_value(encoded).expect("deserialize gap");
        assert_eq!(decoded, gap);
    }
}

#[test]
fn capability_requirement_defaults_mandatory_true() {
    // `mandatory` defaults to true when omitted (fail closed: a requirement is
    // assumed essential unless explicitly marked optional).
    let decoded: CapabilityRequirement = serde_json::from_value(serde_json::json!({
        "requirement_id": "read_source",
        "description": "Read the source file",
        "required_tags": ["file_io"],
    }))
    .expect("deserialize requirement");
    assert!(decoded.mandatory);
    assert_eq!(decoded.requirement_id, "read_source");
}

#[test]
fn strategy_candidate_lifecycle_transitions_fail_closed() {
    use StrategyCandidateStatus::*;

    // Forward path: proposed -> approved -> applied -> superseded.
    assert!(Proposed.can_transition_to(Approved));
    assert!(Approved.can_transition_to(Applied));
    assert!(Applied.can_transition_to(Superseded));

    // Rejection from proposed or approved is allowed.
    assert!(Proposed.can_transition_to(Rejected));
    assert!(Approved.can_transition_to(Rejected));

    // Terminal states accept nothing further.
    assert!(Rejected.is_terminal());
    assert!(Superseded.is_terminal());
    assert!(!Rejected.can_transition_to(Approved));
    assert!(!Superseded.can_transition_to(Applied));

    // Cannot skip review: proposed cannot jump straight to applied.
    assert!(!Proposed.can_transition_to(Applied));
    // Cannot re-open an applied strategy back to approved.
    assert!(!Applied.can_transition_to(Approved));
}

#[test]
fn strategy_candidate_round_trips_json() {
    let candidate = StrategyCandidate {
        candidate_id: "strat_1".to_string(),
        goal_id: "demo_csv_parse".to_string(),
        discovery_decision_id: "gcl_abc123".to_string(),
        composition: CompositionPath {
            sequence: vec!["file_read".to_string(), "csv_parse".to_string()],
            compatibility_score: 0.95,
            reasoning: "Standard pipeline".to_string(),
        },
        status: StrategyCandidateStatus::Proposed,
        confidence: 0.95,
        fingerprint: "fp_xyz".to_string(),
        proposal_draft_id: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };

    let encoded = serde_json::to_value(&candidate).expect("serialize");
    assert_eq!(encoded["status"], "proposed");
    let decoded: StrategyCandidate = serde_json::from_value(encoded).expect("deserialize");
    assert_eq!(decoded, candidate);
}

#[test]
fn workflow_proposal_draft_links_planner_and_preview() {
    let draft = WorkflowProposalDraft {
        proposal_draft_id: "wpd_1".to_string(),
        strategy_candidate_id: "strat_1".to_string(),
        goal_id: "demo_csv_parse".to_string(),
        planner_plan_draft_id: Some("plan_1".to_string()),
        automation_v2_preview_id: Some("auto_preview_1".to_string()),
        required_capabilities: vec!["file_read".to_string(), "csv_parse".to_string()],
        blocked_capabilities: vec![],
        created_at_ms: 7,
    };

    let encoded = serde_json::to_value(&draft).expect("serialize");
    let decoded: WorkflowProposalDraft = serde_json::from_value(encoded).expect("deserialize");
    assert_eq!(decoded, draft);
    assert_eq!(decoded.planner_plan_draft_id.as_deref(), Some("plan_1"));
}

#[test]
fn audit_event_names_are_namespaced() {
    assert!(audit_events::GOAL_PLANNED.starts_with("goal_capability_learning."));
    assert!(audit_events::STRATEGY_PROPOSED.starts_with("goal_capability_learning."));
    assert!(audit_events::PROPOSAL_DRAFTED.starts_with("goal_capability_learning."));
}
