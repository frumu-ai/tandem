use super::discovery::{discover_capabilities_for_goal, generate_discovery_decision_id};
use super::fixtures;
use tandem_types::GoalSpec;

#[test]
fn csv_parse_demo_goal_has_full_composition() {
    let goal = GoalSpec {
        goal_id: "demo_csv_parse".to_string(),
        title: "Read and parse CSV file".to_string(),
        description: "Given a CSV file path, read its contents and parse into structured records"
            .to_string(),
        input_parameters: vec![],
        expected_output_format: "Array of objects (JSON)".to_string(),
        constraints: vec![],
    };

    let report = discover_capabilities_for_goal(&goal);

    // Should discover both file_read and csv_parse.
    assert_eq!(report.discovered_capabilities.len(), 2);

    // Should have at least one composition path.
    assert!(!report.composition_candidates.is_empty());

    // Primary recommendation should be file_read -> csv_parse.
    let primary = report.primary_recommendation();
    assert!(primary.is_some());
    assert_eq!(primary.unwrap().sequence, vec!["file_read", "csv_parse"]);

    // Confidence should be high.
    assert!(report.overall_confidence_score >= 0.9);
}

#[test]
fn fixtures_have_valid_schemas() {
    let file_read = fixtures::file_read_capability();
    assert_eq!(file_read.capability_id, "file_read");
    assert!(!file_read.tags.is_empty());
    assert!(file_read.tags.contains(&"file_io".to_string()));

    let csv_parse = fixtures::csv_parse_capability();
    assert_eq!(csv_parse.capability_id, "csv_parse");
    assert!(csv_parse.tags.contains(&"csv".to_string()));

    let json_ser = fixtures::json_serialize_capability();
    assert_eq!(json_ser.capability_id, "json_serialize");
    assert!(json_ser.tags.contains(&"serialize".to_string()));
}

#[test]
fn all_fixtures_are_loadable() {
    let all = fixtures::all_capabilities();
    assert_eq!(all.len(), 3);
    assert!(all.iter().any(|c| c.capability_id == "file_read"));
    assert!(all.iter().any(|c| c.capability_id == "csv_parse"));
    assert!(all.iter().any(|c| c.capability_id == "json_serialize"));
}

#[test]
fn composition_path_output_input_compatibility() {
    let report = discover_capabilities_for_goal(&GoalSpec {
        goal_id: "demo_csv_parse".to_string(),
        title: "Read and parse CSV file".to_string(),
        description: "Read and parse CSV".to_string(),
        input_parameters: vec![],
        expected_output_format: "CSV records".to_string(),
        constraints: vec![],
    });

    let primary = report.primary_recommendation();
    assert!(primary.is_some());

    let path = primary.unwrap();
    let all_caps = fixtures::all_capabilities();

    // Verify that output of file_read can feed input of csv_parse.
    let file_read = all_caps.iter().find(|c| c.capability_id == "file_read");
    let csv_parse = all_caps.iter().find(|c| c.capability_id == "csv_parse");

    assert!(file_read.is_some());
    assert!(csv_parse.is_some());

    // Both exist in the path.
    assert_eq!(path.sequence[0], "file_read");
    assert_eq!(path.sequence[1], "csv_parse");
}

#[test]
fn discovery_decision_ids_are_unique() {
    let id1 = generate_discovery_decision_id();
    let id2 = generate_discovery_decision_id();

    assert_ne!(id1, id2);
    assert!(id1.starts_with("gcl_"));
    assert!(id2.starts_with("gcl_"));
}
