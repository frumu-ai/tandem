use super::*;

fn demo_goal() -> GoalSpec {
    GoalSpec {
        goal_id: "demo_csv_parse".to_string(),
        title: "Read and parse CSV file".to_string(),
        description: "Given a CSV file path, read its contents and parse into structured records"
            .to_string(),
        input_parameters: vec![GoalParameter {
            name: "file_path".to_string(),
            data_type: "String".to_string(),
            description: "Path to CSV file".to_string(),
            required: true,
        }],
        expected_output_format: "Array of objects (JSON)".to_string(),
        constraints: vec![],
    }
}

#[test]
fn demo_csv_goal_structure_is_valid() {
    let goal = demo_goal();
    assert_eq!(goal.goal_id, "demo_csv_parse");
    assert!(!goal.input_parameters.is_empty());
    assert_eq!(goal.input_parameters[0].name, "file_path");
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
                reasoning: "Wrong format; CSV is not JSON".to_string(),
            },
        ],
        gaps: vec![],
        overall_confidence_score: 0.9,
        reasoning: "Found file_read and csv_parse capabilities".to_string(),
    };

    let primary = report.primary_recommendation();
    assert!(primary.is_some());
    assert_eq!(primary.unwrap().compatibility_score, 0.9);
    assert_eq!(primary.unwrap().sequence[1], "csv_parse");
}

#[test]
fn composition_path_ordering_preserved() {
    let path = CompositionPath {
        sequence: vec![
            "file_read".to_string(),
            "csv_parse".to_string(),
            "json_serialize".to_string(),
        ],
        compatibility_score: 0.95,
        reasoning: "Three-step pipeline".to_string(),
    };

    assert_eq!(path.sequence.len(), 3);
    assert_eq!(path.sequence[0], "file_read");
    assert_eq!(path.sequence[2], "json_serialize");
}

#[test]
fn goal_spec_round_trips_json() {
    let goal = demo_goal();
    let encoded = serde_json::to_value(&goal).expect("serialize goal");
    let decoded: GoalSpec = serde_json::from_value(encoded).expect("deserialize goal");
    assert_eq!(decoded, goal);
}

#[test]
fn discovery_report_round_trips_json() {
    let report = CapabilityDiscoveryReport {
        goal_id: "demo_csv_parse".to_string(),
        discovered_capabilities: vec![AvailableCapability {
            capability_id: "file_read".to_string(),
            tool_name: "FileRead".to_string(),
            input_schema: serde_json::json!({"path": "string"}),
            output_schema: serde_json::json!({"content": "string"}),
            tags: vec!["file_io".to_string()],
        }],
        composition_candidates: vec![CompositionPath {
            sequence: vec!["file_read".to_string(), "csv_parse".to_string()],
            compatibility_score: 0.9,
            reasoning: "Standard pipeline".to_string(),
        }],
        gaps: vec![],
        overall_confidence_score: 0.9,
        reasoning: "Capabilities found".to_string(),
    };

    let encoded = serde_json::to_value(&report).expect("serialize report");
    let decoded: CapabilityDiscoveryReport =
        serde_json::from_value(encoded).expect("deserialize report");
    assert_eq!(decoded, report);
}

#[test]
fn capability_gap_variants_serialize() {
    let gaps = vec![
        CapabilityGap::NotFound {
            description: "No CSV parser found".to_string(),
        },
        CapabilityGap::NotAuthorized {
            capability_id: "secure_file_read".to_string(),
        },
        CapabilityGap::RejectedByConstraint {
            capability_id: "external_api".to_string(),
            reason: "No external APIs allowed".to_string(),
        },
    ];

    for gap in gaps {
        let encoded = serde_json::to_value(&gap).expect("serialize gap");
        let decoded: CapabilityGap = serde_json::from_value(encoded).expect("deserialize gap");
        assert_eq!(decoded, gap);
    }
}

#[test]
fn learning_response_round_trips_json() {
    let response = GoalCapabilityLearningResponse {
        request_id: "req_123".to_string(),
        report: CapabilityDiscoveryReport {
            goal_id: "demo_csv_parse".to_string(),
            discovered_capabilities: vec![],
            composition_candidates: vec![],
            gaps: vec![],
            overall_confidence_score: 0.8,
            reasoning: "Test response".to_string(),
        },
    };

    let encoded = serde_json::to_value(&response).expect("serialize response");
    let decoded: GoalCapabilityLearningResponse =
        serde_json::from_value(encoded).expect("deserialize response");
    assert_eq!(decoded, response);
}
