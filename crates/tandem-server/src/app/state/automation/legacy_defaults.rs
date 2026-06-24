use super::*;

pub(crate) fn automation_node_builder_metadata(
    node: &AutomationFlowNode,
    key: &str,
) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn automation_node_builder_string_array(
    node: &AutomationFlowNode,
    key: &str,
) -> Vec<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get(key))
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
        .unwrap_or_default()
}

pub(crate) fn automation_node_research_stage(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "research_stage")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_is_research_finalize(node: &AutomationFlowNode) -> bool {
    automation_node_research_stage(node).as_deref() == Some("research_finalize")
}

pub(crate) fn automation_node_is_outbound_action(node: &AutomationFlowNode) -> bool {
    if node
        .metadata
        .as_ref()
        .and_then(|value| value.pointer("/builder/role"))
        .and_then(Value::as_str)
        .is_some_and(|role| role.eq_ignore_ascii_case("publisher"))
    {
        return true;
    }
    let objective = node.objective.to_ascii_lowercase();
    [
        "publish",
        "post ",
        "send ",
        "notify",
        "deliver",
        "submit",
        "share",
        "insert ",
        "create page",
        "create database row",
        "update page",
    ]
    .iter()
    .any(|needle| objective.contains(needle))
}

fn automation_node_metadata_bool(node: &AutomationFlowNode, key: &str) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .get(key)
                .or_else(|| metadata.pointer(&format!("/builder/{key}")))
        })
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_forces_full_upstream_inputs(node: &AutomationFlowNode) -> bool {
    automation_node_metadata_bool(node, "preserve_full_upstream_inputs")
        || automation_node_metadata_bool(node, "connector_writer")
}

pub(crate) fn automation_node_uses_upstream_validation_evidence(node: &AutomationFlowNode) -> bool {
    if automation_node_is_research_finalize(node) {
        return true;
    }
    let has_upstream_inputs = !node.input_refs.is_empty() || !node.depends_on.is_empty();
    if !has_upstream_inputs {
        return false;
    }
    if automation_node_forces_full_upstream_inputs(node) {
        return true;
    }
    if automation_node_requires_email_delivery(node) {
        return true;
    }
    let bug_monitor_artifact_type = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.pointer("/bug_monitor/artifact_type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if bug_monitor_artifact_type.is_some_and(|artifact_type| {
        !artifact_type.eq_ignore_ascii_case("bug_monitor_inspection")
            && !artifact_type.eq_ignore_ascii_case("bug_monitor_research")
    }) {
        return true;
    }
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        contract_kind.as_str(),
        "brief" | "report_markdown" | "text_summary" | "review_summary" | "approval_gate"
    )
}

pub(crate) fn automation_node_preserves_full_upstream_inputs(node: &AutomationFlowNode) -> bool {
    if !automation_node_uses_upstream_validation_evidence(node) {
        return false;
    }
    if automation_node_forces_full_upstream_inputs(node) {
        return true;
    }
    matches!(
        node.output_contract
            .as_ref()
            .map(|contract| contract.kind.trim().to_ascii_lowercase())
            .as_deref(),
        Some("report_markdown" | "text_summary")
    ) || automation_node_requires_email_delivery(node)
}

pub(crate) fn automation_node_delivery_method(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/method")
                .or_else(|| value.pointer("/builder/delivery/method"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

pub(crate) fn automation_node_delivery_target(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/to")
                .or_else(|| value.pointer("/builder/delivery/to"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| extract_email_address_from_text(&node.objective))
}

pub(crate) fn extract_email_address_from_text(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|ch: char| {
                ch.is_ascii_punctuation() && ch != '@' && ch != '.' && ch != '_' && ch != '-'
            })
            .trim();
        if candidate.is_empty()
            || !candidate.contains('@')
            || candidate.starts_with('@')
            || candidate.ends_with('@')
        {
            return None;
        }
        let mut parts = candidate.split('@');
        let local = parts.next()?.trim();
        let domain = parts.next()?.trim();
        if parts.next().is_some()
            || local.is_empty()
            || domain.is_empty()
            || !domain.contains('.')
            || domain.starts_with('.')
            || domain.ends_with('.')
        {
            return None;
        }
        Some(candidate.to_string())
    })
}

pub(crate) fn automation_node_email_content_type(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/content_type")
                .or_else(|| value.pointer("/builder/delivery/content_type"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn automation_node_inline_body_only(node: &AutomationFlowNode) -> Option<bool> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/inline_body_only")
                .or_else(|| value.pointer("/builder/delivery/inline_body_only"))
        })
        .and_then(Value::as_bool)
}

pub(crate) fn automation_node_allows_attachments(node: &AutomationFlowNode) -> Option<bool> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/attachments")
                .or_else(|| value.pointer("/builder/delivery/attachments"))
        })
        .and_then(Value::as_bool)
}

pub(crate) fn automation_node_requires_email_delivery(node: &AutomationFlowNode) -> bool {
    if automation_node_delivery_method(node)
        .as_deref()
        .is_some_and(|method| method == "email")
    {
        return true;
    }
    if !automation_node_is_outbound_action(node) {
        return false;
    }
    if automation_node_delivery_target(node).is_some() {
        return true;
    }
    let objective = node.objective.to_ascii_lowercase();
    let contains_phrase = [
        "send email",
        "send the email",
        "send by email",
        "send the report by email",
        "email the ",
        "email report",
        "draft email",
        "draft the email",
        "gmail draft",
        "gmail_send",
        "notify by email",
        "notify the operator by email",
    ]
    .iter()
    .any(|needle| objective.contains(needle));
    if contains_phrase {
        return true;
    }
    false
}
