// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(crate) fn automation_node_required_status_marker(node: &AutomationFlowNode) -> Option<&str> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.pointer("/artifact/required_status_marker"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|marker| !marker.is_empty())
}
