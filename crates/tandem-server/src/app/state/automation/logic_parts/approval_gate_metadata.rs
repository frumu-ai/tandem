// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn automation_node_uses_metadata_approval_gate(node: &AutomationFlowNode) -> bool {
    let Some(metadata) = node.metadata.as_ref() else {
        return false;
    };
    let metadata_stage_kind = metadata
        .get("stage_kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("approval"));
    let human_decision_task = metadata
        .pointer("/builder/task_class")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("human_decision_gate"));
    let explicit_approval_metadata = metadata
        .get("approval")
        .and_then(Value::as_object)
        .is_some_and(|approval| {
            approval.contains_key("allowed_decisions")
                || approval.contains_key("decisions")
                || approval
                    .get("require_explicit_decision")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        });
    metadata_stage_kind || human_decision_task || explicit_approval_metadata
}

fn metadata_automation_gate_parts(
    node: &AutomationFlowNode,
) -> Option<(
    Option<String>,
    Vec<String>,
    Vec<String>,
    Option<crate::AutomationGateExpiryPolicy>,
)> {
    if !automation_node_uses_metadata_approval_gate(node) {
        return None;
    }
    let approval = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("approval"));
    let mut decisions = approval
        .and_then(|approval| {
            approval
                .get("allowed_decisions")
                .or_else(|| approval.get("decisions"))
        })
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if decisions.is_empty() {
        decisions = vec!["Approve".to_string(), "Reject".to_string()];
    }
    let rework_targets = approval
        .and_then(|approval| approval.get("rework_targets"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let instructions = approval
        .and_then(|approval| approval.get("instructions"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| Some(node.objective.clone()));
    Some((
        instructions,
        decisions,
        rework_targets,
        automation_gate_expiry_policy_from_node_metadata(node),
    ))
}

fn automation_pending_gate_metadata(node: &AutomationFlowNode) -> Option<Value> {
    let approval = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("approval"))?;
    let mut metadata = match approval.get("metadata").cloned() {
        Some(Value::Object(object)) => object,
        Some(value) => serde_json::Map::from_iter([("gate_metadata".to_string(), value)]),
        None => serde_json::Map::new(),
    };
    for key in ["display_artifact", "record_reviewed_artifact_hash"] {
        if let Some(value) = approval.get(key) {
            metadata.insert(key.to_string(), value.clone());
        }
    }
    (!metadata.is_empty()).then_some(Value::Object(metadata))
}
