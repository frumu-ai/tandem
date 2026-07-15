// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use serde_json::json;

#[test]
fn automation_node_retry_policy_uses_legacy_max_attempts_field() {
    let mut node = AutomationNodeBuilder::new("retry").build();
    node.retry_policy = Some(json!({
        "max_attempts": 4
    }));

    assert_eq!(automation_node_max_attempts(&node), 4);
}

#[test]
fn automation_node_retry_policy_clamps_legacy_max_attempts() {
    let mut node = AutomationNodeBuilder::new("retry").build();
    node.retry_policy = Some(json!({
        "max_attempts": 0
    }));

    assert_eq!(automation_node_max_attempts(&node), 1);

    node.retry_policy = Some(json!({
        "max_attempts": 99
    }));

    assert_eq!(automation_node_max_attempts(&node), 10);
}

#[test]
fn automation_node_retry_policy_preserves_research_default_attempts() {
    let node = AutomationNodeBuilder::new("research")
        .output_contract(AutomationFlowOutputContract {
            kind: "artifact".to_string(),
            validator: Some(AutomationOutputValidatorKind::ResearchBrief),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        })
        .build();

    assert_eq!(automation_node_max_attempts(&node), 5);
}
