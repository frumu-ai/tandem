// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::decision_store::GoalCapabilityLearningDecisionStore;
use super::discovery::discover_capabilities_for_goal;
use tandem_types::GoalSpec;

#[test]
fn csv_demo_goal_discovery() {
    let goal = GoalSpec {
        goal_id: "demo_csv_parse".to_string(),
        title: "Read and parse CSV file".to_string(),
        description: "Given a CSV file path, read and parse into records".to_string(),
        input_parameters: vec![],
        expected_output_format: "Array of objects (JSON)".to_string(),
        constraints: vec![],
    };

    let report = discover_capabilities_for_goal(&goal);

    assert_eq!(report.goal_id, "demo_csv_parse");
    assert_eq!(report.discovered_capabilities.len(), 2);
    assert!(!report.composition_candidates.is_empty());

    let primary = report.primary_recommendation();
    assert!(primary.is_some());
    assert_eq!(primary.unwrap().sequence, vec!["file_read", "csv_parse"]);
    assert!(report.overall_confidence_score >= 0.9);
}

#[test]
fn unrecognized_goal_has_low_confidence() {
    let goal = GoalSpec {
        goal_id: "unknown".to_string(),
        title: "Unknown operation".to_string(),
        description: "Something we don't recognize".to_string(),
        input_parameters: vec![],
        expected_output_format: "Unknown".to_string(),
        constraints: vec![],
    };

    let report = discover_capabilities_for_goal(&goal);

    assert!(report.composition_candidates.is_empty());
    assert!(report.overall_confidence_score < 0.5);
}

#[tokio::test]
async fn discovery_store_persists_decisions() {
    let store = GoalCapabilityLearningDecisionStore::new();
    let goal = GoalSpec {
        goal_id: "demo".to_string(),
        title: "Read and parse CSV".to_string(),
        description: "CSV parsing".to_string(),
        input_parameters: vec![],
        expected_output_format: "Records".to_string(),
        constraints: vec![],
    };

    let response = store
        .discover_for_goal(goal.clone(), "tenant_abc".to_string())
        .await;

    assert!(response.request_id.starts_with("gcl_"));
    assert_eq!(response.report.goal_id, "demo");

    let retrieved = store.get_decision(&response.request_id).await;
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().tenant_id, "tenant_abc");
}

#[tokio::test]
async fn tenant_isolation_in_store() {
    let store = GoalCapabilityLearningDecisionStore::new();
    let goal = GoalSpec {
        goal_id: "demo".to_string(),
        title: "Read and parse CSV".to_string(),
        description: "CSV parsing".to_string(),
        input_parameters: vec![],
        expected_output_format: "Records".to_string(),
        constraints: vec![],
    };

    store
        .discover_for_goal(goal.clone(), "tenant_1".to_string())
        .await;
    store
        .discover_for_goal(goal.clone(), "tenant_1".to_string())
        .await;
    store.discover_for_goal(goal, "tenant_2".to_string()).await;

    let t1_decisions = store.list_for_tenant("tenant_1").await;
    let t2_decisions = store.list_for_tenant("tenant_2").await;

    assert_eq!(t1_decisions.len(), 2);
    assert_eq!(t2_decisions.len(), 1);
    assert!(t1_decisions.iter().all(|d| d.tenant_id == "tenant_1"));
    assert!(t2_decisions.iter().all(|d| d.tenant_id == "tenant_2"));
}
