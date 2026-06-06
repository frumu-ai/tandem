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
