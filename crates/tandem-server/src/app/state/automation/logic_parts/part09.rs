const CONNECTOR_CAPTURE_SCHEMA_VERSION: u32 = 1;
const CONNECTOR_CAPTURE_ITEM_LIMIT: usize = 5_000;

fn automation_connector_capture_metadata_value(node: &AutomationFlowNode) -> Option<&Value> {
    node.metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .get("connector_capture")
                .or_else(|| metadata.pointer("/builder/connector_capture"))
                .or_else(|| metadata.get("connector_result_capture"))
                .or_else(|| metadata.pointer("/builder/connector_result_capture"))
        })
}

fn automation_connector_capture_is_disabled(node: &AutomationFlowNode) -> bool {
    match automation_connector_capture_metadata_value(node) {
        Some(Value::Bool(false)) => true,
        Some(Value::Object(map)) => map
            .get("enabled")
            .and_then(Value::as_bool)
            .is_some_and(|enabled| !enabled),
        _ => false,
    }
}

fn automation_connector_capture_is_explicit(node: &AutomationFlowNode) -> bool {
    match automation_connector_capture_metadata_value(node) {
        Some(Value::Bool(true)) => true,
        Some(Value::Object(map)) => map
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        _ => false,
    }
}

fn automation_connector_capture_tool_patterns(node: &AutomationFlowNode) -> Vec<String> {
    let mut patterns = Vec::new();
    let Some(value) = automation_connector_capture_metadata_value(node) else {
        return patterns;
    };
    let Some(map) = value.as_object() else {
        return patterns;
    };
    for key in ["tools", "source_tools", "capture_tools", "tool_allowlist"] {
        let Some(rows) = map.get(key).and_then(Value::as_array) else {
            continue;
        };
        patterns.extend(
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_ascii_lowercase()),
        );
    }
    patterns.sort();
    patterns.dedup();
    patterns
}

fn automation_connector_capture_text_suggests_collection(node: &AutomationFlowNode) -> bool {
    let mut text = format!("{} {}", node.node_id, node.objective).to_ascii_lowercase();
    if let Some(metadata) = node.metadata.as_ref() {
        text.push(' ');
        text.push_str(&metadata.to_string().to_ascii_lowercase());
    }
    if !tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(&text) {
        return false;
    }
    [
        "collect",
        "extract",
        "search",
        "query",
        "fetch",
        "retrieve",
        "scan",
        "gather",
        "harvest",
        "find",
        "list",
        "source",
        "research",
        "lead",
        "signal",
        "candidate",
        "thread",
        "post",
        "issue",
        "ticket",
        "record",
        "dataset",
        "results",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn automation_connector_capture_pattern_matches(tool: &str, pattern: &str) -> bool {
    let tool = tool.trim().to_ascii_lowercase();
    let pattern = pattern.trim().to_ascii_lowercase();
    if pattern.is_empty() {
        return false;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        tool == prefix || tool.starts_with(&format!("{prefix}."))
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        tool.starts_with(prefix)
    } else {
        tool == pattern
    }
}

fn automation_connector_capture_tool_looks_mutating(normalized_tool: &str) -> bool {
    let leaf = normalized_tool
        .rsplit('.')
        .next()
        .unwrap_or(normalized_tool)
        .trim();
    [
        "create",
        "update",
        "delete",
        "write",
        "send",
        "post",
        "submit",
        "insert",
        "upsert",
        "patch",
        "remove",
        "archive",
        "replace",
        "publish",
        "draft",
        "compose",
        "manage",
        "connect",
        "disconnect",
        "auth",
        "oauth",
    ]
    .iter()
    .any(|prefix| {
        leaf == *prefix
            || leaf.starts_with(&format!("{prefix}_"))
            || leaf.starts_with(&format!("{prefix}-"))
            || leaf.contains(&format!("_{prefix}_"))
            || leaf.contains(&format!("-{prefix}-"))
    })
}

fn automation_connector_capture_tool_is_candidate(
    node: &AutomationFlowNode,
    normalized_tool: &str,
    explicit_patterns: &[String],
    explicit_capture: bool,
) -> bool {
    if normalized_tool == "mcp_list"
        || normalized_tool == "mcp_list_catalog"
        || normalized_tool == "mcp_request_capability"
        || !normalized_tool.starts_with("mcp.")
    {
        return false;
    }
    let explicit_pattern_matched = explicit_patterns
        .iter()
        .any(|pattern| automation_connector_capture_pattern_matches(normalized_tool, pattern));
    if explicit_pattern_matched {
        return true;
    }
    if !explicit_patterns.is_empty() {
        return false;
    }
    if explicit_capture {
        return true;
    }
    if automation_connector_capture_tool_looks_mutating(normalized_tool) {
        return false;
    }
    if automation_node_is_outbound_action(node) {
        return false;
    }
    automation_connector_capture_text_suggests_collection(node)
}

fn automation_connector_capture_result_is_usable(result: Option<&Value>) -> bool {
    result.is_some() && automation_tool_result_failure_reason(result).is_none()
}

fn automation_connector_capture_slug(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').trim_matches('.');
    if trimmed.is_empty() {
        "connector-results".to_string()
    } else {
        trimmed.to_string()
    }
}

fn automation_connector_capture_artifact_path(run_id: &str, node_id: &str) -> String {
    format!(
        ".tandem/runs/{}/artifacts/{}-connector-results.json",
        automation_connector_capture_slug(run_id),
        automation_connector_capture_slug(node_id)
    )
}

fn automation_connector_capture_collect_items_from_payload(
    value: &Value,
    items: &mut Vec<Value>,
    truncated: &mut bool,
    depth: usize,
) {
    if items.len() >= CONNECTOR_CAPTURE_ITEM_LIMIT {
        *truncated = true;
        return;
    }
    if depth > 5 {
        return;
    }
    match value {
        Value::Array(rows) => {
            let object_like = rows
                .iter()
                .any(|row| matches!(row, Value::Object(_) | Value::Array(_)));
            if object_like {
                for row in rows {
                    if items.len() >= CONNECTOR_CAPTURE_ITEM_LIMIT {
                        *truncated = true;
                        return;
                    }
                    items.push(row.clone());
                }
            }
        }
        Value::Object(map) => {
            for key in [
                "results",
                "items",
                "data",
                "records",
                "rows",
                "posts",
                "threads",
                "comments",
                "documents",
                "entries",
                "hits",
                "values",
            ] {
                if let Some(child) = map.get(key) {
                    match child {
                        Value::Array(_) => automation_connector_capture_collect_items_from_payload(
                            child,
                            items,
                            truncated,
                            depth + 1,
                        ),
                        Value::Object(_) => automation_connector_capture_collect_items_from_payload(
                            child,
                            items,
                            truncated,
                            depth + 1,
                        ),
                        _ => {}
                    }
                }
                if *truncated {
                    return;
                }
            }
            if items.is_empty() {
                for child in map.values() {
                    automation_connector_capture_collect_items_from_payload(
                        child,
                        items,
                        truncated,
                        depth + 1,
                    );
                    if *truncated || !items.is_empty() {
                        break;
                    }
                }
            }
        }
        _ => {}
    }
}

fn automation_connector_capture_result_payload(result: Option<&Value>) -> Value {
    automation_tool_result_output_payload(result).unwrap_or(Value::Null)
}

pub(crate) fn persist_automation_connector_tool_result_capture(
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
) -> anyhow::Result<Option<Value>> {
    if automation_connector_capture_is_disabled(node) {
        return Ok(None);
    }
    let explicit_capture = automation_connector_capture_is_explicit(node);
    let explicit_patterns = automation_connector_capture_tool_patterns(node);
    let mut captured_results = Vec::new();
    let mut extracted_items = Vec::new();
    let mut extracted_items_truncated = false;

    for (call_index, part) in session
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .enumerate()
    {
        let MessagePart::ToolInvocation {
            tool,
            args,
            result,
            error,
        } = part
        else {
            continue;
        };
        if error.as_ref().is_some_and(|value| !value.trim().is_empty())
            || !automation_connector_capture_result_is_usable(result.as_ref())
        {
            continue;
        }
        let normalized_tool = tool.trim().to_ascii_lowercase().replace('-', "_");
        if !automation_connector_capture_tool_is_candidate(
            node,
            &normalized_tool,
            &explicit_patterns,
            explicit_capture,
        ) {
            continue;
        }
        let payload = automation_connector_capture_result_payload(result.as_ref());
        automation_connector_capture_collect_items_from_payload(
            &payload,
            &mut extracted_items,
            &mut extracted_items_truncated,
            0,
        );
        captured_results.push(json!({
            "call_index": call_index,
            "tool": tool,
            "normalized_tool": normalized_tool,
            "args": args,
            "result": result,
            "output_payload": payload,
            "metadata": automation_tool_result_metadata(result.as_ref()).cloned().unwrap_or(Value::Null),
        }));
    }

    if captured_results.is_empty() {
        return Ok(None);
    }

    let relative_path = automation_connector_capture_artifact_path(run_id, &node.node_id);
    let resolved = resolve_automation_output_path(workspace_root, &relative_path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tools = captured_results
        .iter()
        .filter_map(|row| row.get("tool").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let payload = json!({
        "artifact_kind": "connector_tool_results",
        "schema_version": CONNECTOR_CAPTURE_SCHEMA_VERSION,
        "automation_id": automation.automation_id,
        "run_id": run_id,
        "node_id": node.node_id,
        "created_at_ms": now_ms(),
        "capture_source": if explicit_capture { "explicit_metadata" } else { "auto_connector_source" },
        "tool_result_count": captured_results.len(),
        "tools": tools,
        "extracted_item_count": extracted_items.len(),
        "extracted_items_truncated": extracted_items_truncated,
        "extracted_items": extracted_items,
        "results": captured_results,
    });
    let serialized = serde_json::to_string_pretty(&payload)?;
    let digest = sha256_hex(&[&serialized]);
    std::fs::write(&resolved, serialized)?;
    let summary = json!({
        "artifact_kind": "connector_tool_results",
        "schema_version": CONNECTOR_CAPTURE_SCHEMA_VERSION,
        "artifact_path": relative_path,
        "tool_result_count": payload.get("tool_result_count").cloned().unwrap_or(json!(0)),
        "tools": payload.get("tools").cloned().unwrap_or_else(|| json!([])),
        "extracted_item_count": payload.get("extracted_item_count").cloned().unwrap_or(json!(0)),
        "extracted_items_truncated": payload.get("extracted_items_truncated").cloned().unwrap_or(json!(false)),
        "content_digest": digest,
    });
    Ok(Some(summary))
}

pub(crate) fn attach_automation_connector_capture_to_output(output: &mut Value, capture: &Value) {
    let Some(object) = output.as_object_mut() else {
        return;
    };
    object.insert("connector_capture".to_string(), capture.clone());
    let artifact_path = capture
        .get("artifact_path")
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(path) = artifact_path {
        let refs = object
            .entry("artifact_refs".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(rows) = refs.as_array_mut() {
            if !rows.iter().any(|row| row.as_str() == Some(path.as_str())) {
                rows.push(json!(path));
            }
        }
    }
    if let Some(content) = object.get_mut("content").and_then(Value::as_object_mut) {
        content.insert("connector_capture".to_string(), capture.clone());
    }
}

#[cfg(test)]
mod connector_capture_tests {
    use super::*;

    fn capture_node() -> AutomationFlowNode {
        AutomationFlowNode {
            node_id: "search_reddit".to_string(),
            agent_id: "researcher".to_string(),
            objective:
                "Use Reddit MCP to search and collect lead candidates from connector-backed source research."
                    .to_string(),
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            tool_policy: None,
            mcp_policy: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            metadata: None,
        }
    }

    #[test]
    fn connector_capture_collects_composio_multi_execute_for_source_node() {
        let node = capture_node();
        assert!(automation_connector_capture_tool_is_candidate(
            &node,
            "mcp.composio_gmail.composio_multi_execute_tool",
            &[],
            false
        ));
    }

    #[test]
    fn connector_capture_skips_outbound_node_without_explicit_metadata() {
        let mut node = capture_node();
        node.metadata = Some(json!({
            "delivery": {
                "method": "email",
                "to": "ops@example.com"
            }
        }));
        assert!(!automation_connector_capture_tool_is_candidate(
            &node,
            "mcp.notion.notion_update_page",
            &[],
            false
        ));
    }

    #[test]
    fn connector_capture_respects_explicit_tool_allowlist() {
        let mut node = capture_node();
        node.metadata = Some(json!({
            "connector_capture": {
                "enabled": true,
                "tools": ["mcp.reddit.*"]
            }
        }));
        let patterns = automation_connector_capture_tool_patterns(&node);
        assert!(automation_connector_capture_tool_is_candidate(
            &node,
            "mcp.reddit.search",
            &patterns,
            true
        ));
        assert!(!automation_connector_capture_tool_is_candidate(
            &node,
            "mcp.notion.notion_fetch",
            &patterns,
            true
        ));
    }

    #[test]
    fn connector_capture_skips_encoded_failure_results() {
        let failure = json!({
            "output": {
                "status": 500,
                "message": "upstream connector failed"
            }
        });
        let success = json!({
            "output": {
                "results": [{"id": "lead-1"}]
            }
        });

        assert!(!automation_connector_capture_result_is_usable(Some(&failure)));
        assert!(automation_connector_capture_result_is_usable(Some(&success)));
    }

    #[test]
    fn connector_capture_extracts_nested_items() {
        let payload = json!({
            "data": {
                "results": [
                    {"id": "a"},
                    {"id": "b"}
                ]
            }
        });
        let mut items = Vec::new();
        let mut truncated = false;
        automation_connector_capture_collect_items_from_payload(
            &payload,
            &mut items,
            &mut truncated,
            0,
        );
        assert_eq!(items.len(), 2);
        assert!(!truncated);
    }
}
