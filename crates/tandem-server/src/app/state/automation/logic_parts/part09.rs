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

fn automation_connector_capture_collect_preferred_result_items(
    value: &Value,
    items: &mut Vec<Value>,
    truncated: &mut bool,
    depth: usize,
) -> bool {
    let before = items.len();
    for pointer in [
        "/response/data",
        "/response/data_preview",
        "/response/data/posts",
        "/response/data_preview/posts",
        "/data/posts",
        "/data_preview/posts",
    ] {
        if let Some(child) = value.pointer(pointer) {
            automation_connector_capture_collect_items_from_payload(
                child,
                items,
                truncated,
                depth + 1,
            );
            if *truncated || items.len() > before {
                return true;
            }
        }
    }
    items.len() > before
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
                    if automation_connector_capture_collect_preferred_result_items(
                        row,
                        items,
                        truncated,
                        depth + 1,
                    ) {
                        if *truncated {
                            return;
                        }
                        continue;
                    }
                    if matches!(row, Value::Object(_) | Value::Array(_)) {
                        items.push(row.clone());
                    } else if row.as_str().is_some_and(
                        automation_connector_capture_string_is_truncation_marker,
                    ) {
                        *truncated = true;
                        return;
                    }
                }
            }
        }
        Value::Object(map) => {
            if automation_connector_capture_collect_preferred_result_items(
                value,
                items,
                truncated,
                depth + 1,
            ) {
                return;
            }
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

fn automation_connector_capture_string_is_truncation_marker(value: &str) -> bool {
    let trimmed = value.trim().to_ascii_lowercase();
    trimmed.starts_with("...")
        && (trimmed.contains("more item")
            || trimmed.contains("more result")
            || trimmed.contains("more row")
            || trimmed.contains("more post")
            || trimmed.contains("truncated"))
}

fn automation_connector_capture_result_payload(result: Option<&Value>) -> Value {
    automation_tool_result_output_payload(result).unwrap_or(Value::Null)
}

fn automation_connector_capture_collect_remote_file_info(
    value: &Value,
    remote_files: &mut Vec<Value>,
    depth: usize,
) {
    if depth > 6 {
        return;
    }
    match value {
        Value::Object(map) => {
            if let Some(info) = map.get("remote_file_info").filter(|info| info.is_object()) {
                remote_files.push(info.clone());
            }
            if map
                .get("file_path")
                .and_then(Value::as_str)
                .is_some_and(|path| path.starts_with("/mnt/files/"))
                && map
                    .get("instructions")
                    .or_else(|| map.get("message"))
                    .is_some()
            {
                remote_files.push(value.clone());
            }
            for (key, child) in map {
                if key == "remote_file_info" {
                    continue;
                }
                automation_connector_capture_collect_remote_file_info(
                    child,
                    remote_files,
                    depth + 1,
                );
            }
        }
        Value::Array(rows) => {
            for row in rows {
                automation_connector_capture_collect_remote_file_info(row, remote_files, depth + 1);
            }
        }
        _ => {}
    }
}

fn automation_connector_capture_tool_is_composio_remote_reader(normalized_tool: &str) -> bool {
    normalized_tool.ends_with(".composio_remote_bash_tool")
        || normalized_tool.ends_with(".composio_remote_workbench")
}

fn automation_connector_capture_remote_file_paths(remote_files: &[Value]) -> Vec<String> {
    remote_files
        .iter()
        .filter_map(|value| value.get("file_path").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| value.starts_with("/mnt/files/"))
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn automation_connector_capture_value_mentions_any_remote_path(
    value: &Value,
    remote_file_paths: &[String],
) -> bool {
    if remote_file_paths.is_empty() {
        return false;
    }
    let text = value.to_string();
    remote_file_paths.iter().any(|path| text.contains(path))
}

fn automation_connector_capture_exact_remote_paths_from_value(value: &Value) -> Vec<String> {
    let text = value.to_string();
    let mut paths = Vec::new();
    for (index, _) in text.match_indices("/mnt/files/") {
        let tail = &text[index..];
        let end = tail
            .find(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '"' | '`' | '\'' | '\\' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
                    )
            })
            .unwrap_or(tail.len());
        let path = tail[..end].trim_matches(|ch: char| matches!(ch, '.' | ':' | '!'));
        let path_suffix = path.trim_start_matches("/mnt/files/");
        let first_segment = path_suffix.split('/').next().unwrap_or_default();
        if path.starts_with("/mnt/files/")
            && !path.contains('*')
            && path_suffix.contains('/')
            && !first_segment.starts_with('.')
            && path.contains('.')
            && !paths.iter().any(|seen| seen == path)
        {
            paths.push(path.to_string());
        }
    }
    paths
}

fn automation_connector_capture_reader_args_remote_paths(evidence: &Value) -> Vec<String> {
    evidence
        .get("args")
        .map(automation_connector_capture_exact_remote_paths_from_value)
        .unwrap_or_default()
}

fn automation_connector_capture_reader_args_mention_any_remote_path(
    evidence: &Value,
    remote_file_paths: &[String],
) -> bool {
    evidence
        .get("args")
        .is_some_and(|args| automation_connector_capture_value_mentions_any_remote_path(args, remote_file_paths))
}

fn automation_connector_capture_matched_remote_paths(
    read_paths: &[String],
    remote_file_paths: &[String],
) -> Vec<String> {
    read_paths
        .iter()
        .filter(|path| remote_file_paths.iter().any(|remote_path| remote_path == *path))
        .cloned()
        .collect()
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
    let mut remote_files = Vec::new();
    let mut remote_reader_evidence = Vec::new();

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
        automation_connector_capture_collect_remote_file_info(&payload, &mut remote_files, 0);
        if automation_connector_capture_tool_is_composio_remote_reader(&normalized_tool) {
            remote_reader_evidence.push(json!({
                "args": args,
                "output_payload": payload,
            }));
        }
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
    remote_files.sort_by(|left, right| left.to_string().cmp(&right.to_string()));
    remote_files.dedup();
    let remote_file_count = remote_files.len();
    let remote_file_paths = automation_connector_capture_remote_file_paths(&remote_files);
    let remote_reader_read_paths = remote_reader_evidence
        .iter()
        .flat_map(automation_connector_capture_reader_args_remote_paths)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let remote_reader_matched_paths = automation_connector_capture_matched_remote_paths(
        &remote_reader_read_paths,
        &remote_file_paths,
    );
    let remote_hydration_performed = !remote_reader_matched_paths.is_empty();
    let remote_hydration_required = remote_file_count > 0 && !remote_hydration_performed;

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
        "remote_file_count": remote_file_count,
        "remote_file_paths": remote_file_paths,
        "remote_files": remote_files,
        "remote_reader_read_paths": remote_reader_read_paths,
        "remote_reader_matched_paths": remote_reader_matched_paths,
        "remote_hydration_required": remote_hydration_required,
        "remote_hydration_performed": remote_hydration_performed,
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
        "remote_file_count": payload.get("remote_file_count").cloned().unwrap_or(json!(0)),
        "remote_file_paths": remote_file_paths,
        "remote_files": payload.get("remote_files").cloned().unwrap_or_else(|| json!([])),
        "remote_reader_read_paths": payload.get("remote_reader_read_paths").cloned().unwrap_or_else(|| json!([])),
        "remote_reader_matched_paths": payload.get("remote_reader_matched_paths").cloned().unwrap_or_else(|| json!([])),
        "remote_hydration_required": payload.get("remote_hydration_required").cloned().unwrap_or(json!(false)),
        "remote_hydration_performed": payload.get("remote_hydration_performed").cloned().unwrap_or(json!(false)),
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

pub(crate) fn apply_automation_connector_capture_validation_metadata(
    artifact_validation: &mut Value,
    capture: &Value,
    attempt: u32,
    max_attempts: u32,
    _source_artifact_materialized: bool,
) {
    let Some(object) = artifact_validation.as_object_mut() else {
        return;
    };
    object.insert("connector_capture".to_string(), capture.clone());
    if let Some(path) = capture.get("artifact_path").and_then(Value::as_str) {
        object.insert("connector_capture_artifact_path".to_string(), json!(path));
    }
    let remote_hydration_required = capture
        .get("remote_hydration_required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !remote_hydration_required {
        return;
    }
    let unmet = object
        .entry("unmet_requirements".to_string())
        .or_insert_with(|| json!([]));
    if let Some(rows) = unmet.as_array_mut() {
        if !rows.iter().any(|value| {
            value.as_str() == Some("connector_remote_result_not_materialized")
        }) {
            rows.push(json!("connector_remote_result_not_materialized"));
        }
    }
    object.insert(
        "semantic_block_reason".to_string(),
        json!("connector returned a remote result file, but the workflow artifact was finalized before materializing it"),
    );
    object.insert(
        "validation_outcome".to_string(),
        json!(if attempt < max_attempts {
            "needs_repair"
        } else {
            "blocked"
        }),
    );
    object.insert(
        "rejected_artifact_reason".to_string(),
        json!("connector remote result file was not read through the available remote helper before writing the artifact"),
    );
    let remote_file_paths = capture
        .get("remote_file_paths")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !remote_file_paths.is_empty() {
        object.insert(
            "connector_remote_file_paths".to_string(),
            json!(remote_file_paths.clone()),
        );
    }
    let actions = object
        .entry("required_next_tool_actions".to_string())
        .or_insert_with(|| json!([]));
    if let Some(rows) = actions.as_array_mut() {
        let exact_file_instruction = if remote_file_paths.is_empty() {
            "Use the available remote bash/workbench helper to read the exact connector remote result file, then write the required artifact from that file rather than from stale sandbox artifacts or previews.".to_string()
        } else {
            format!(
                "Use the available remote bash/workbench helper to read the exact connector remote result file(s): {}. The helper command/code must reference these path(s) directly; do not read similarly named stale files such as `/mnt/files/<node>.json`.",
                remote_file_paths.join(", ")
            )
        };
        if !rows
            .iter()
            .any(|value| value.as_str() == Some(exact_file_instruction.as_str()))
        {
            rows.push(json!(exact_file_instruction));
        }
    }
}

pub(crate) fn automation_required_connector_capture_read_paths(
    tool_telemetry: &Value,
) -> Vec<String> {
    tool_telemetry
        .get("required_connector_capture_read_paths")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|rows| rows.iter())
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn annotate_automation_connector_capture_read_requirements(
    tool_telemetry: &mut Value,
    upstream_inputs: &[Value],
) {
    let paths =
        crate::app::state::automation::prompting_impl::automation_prompt_upstream_connector_capture_artifact_paths(
            upstream_inputs,
        );
    if paths.is_empty() {
        return;
    }
    if let Some(object) = tool_telemetry.as_object_mut() {
        object.insert(
            "required_connector_capture_read_paths".to_string(),
            json!(paths),
        );
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

    #[test]
    fn connector_capture_extracts_composio_preview_records_not_wrapper() {
        let payload = json!({
            "data": {
                "results": [
                    {
                        "tool_slug": "REDDIT_SEARCH_ACROSS_SUBREDDITS",
                        "response": {
                            "successful": true,
                            "data_preview": {
                                "posts": [
                                    {"id": "a", "title": "first"},
                                    {"id": "b", "title": "second"},
                                    "...8 more items"
                                ],
                                "total_results": 10
                            }
                        }
                    }
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
        assert_eq!(items[0].get("id").and_then(Value::as_str), Some("a"));
        assert_eq!(items[1].get("id").and_then(Value::as_str), Some("b"));
        assert!(truncated);
    }

    #[test]
    fn connector_capture_detects_composio_remote_file_info() {
        let payload = json!({
            "data": {
                "remote_file_info": {
                    "file_path": "/mnt/files/mex/pour.json",
                    "instructions": "Use COMPOSIO_REMOTE_BASH_TOOL"
                }
            }
        });
        let mut remote_files = Vec::new();
        automation_connector_capture_collect_remote_file_info(&payload, &mut remote_files, 0);
        assert_eq!(remote_files.len(), 1);
        assert_eq!(
            remote_files[0].get("file_path").and_then(Value::as_str),
            Some("/mnt/files/mex/pour.json")
        );
    }

    #[test]
    fn connector_capture_remote_hydration_requires_referenced_remote_file() {
        let remote_files = vec![json!({
            "file_path": "/mnt/files/mex/pour.json",
            "instructions": "Use COMPOSIO_REMOTE_BASH_TOOL"
        })];
        let remote_file_paths = automation_connector_capture_remote_file_paths(&remote_files);

        let sandbox_only_write = json!({
            "args": {
                "command": "mkdir -p .tandem/runs/run/artifacts && cat > .tandem/runs/run/artifacts/search.json"
            },
            "output_payload": {
                "data": {
                    "stdout": "VERIFIED\n"
                }
            }
        });
        assert!(
            !automation_connector_capture_value_mentions_any_remote_path(
                &sandbox_only_write,
                &remote_file_paths
            ),
            "a remote helper call must not count as hydration unless it reads the remote result file"
        );

        let remote_file_list_only = json!({
            "args": {
                "command": "find /mnt/files -maxdepth 5 -type f"
            },
            "output_payload": {
                "data": {
                    "stdout": "/mnt/files/mex/pour.json\n"
                }
            }
        });
        assert!(
            !automation_connector_capture_reader_args_mention_any_remote_path(
                &remote_file_list_only,
                &remote_file_paths
            ),
            "listing a remote result file path is not enough; the helper must read that exact file"
        );

        let remote_file_read = json!({
            "args": {
                "command": "jq '.results[0].response.data.posts' /mnt/files/mex/pour.json"
            },
            "output_payload": {
                "data": {
                    "stdout": "[{\"id\":\"lead-1\"}]"
                }
            }
        });
        assert!(automation_connector_capture_value_mentions_any_remote_path(
            &remote_file_read,
            &remote_file_paths
        ));
        assert!(automation_connector_capture_reader_args_mention_any_remote_path(
            &remote_file_read,
            &remote_file_paths
        ));

        let prior_remote_file_read = json!({
            "args": {
                "command": "python3 - <<'PY'\nimport json\njson.load(open('/mnt/files/mex/seen.json'))\nPY"
            },
            "output_payload": {
                "data": {
                    "stdout": "{\"raw_posts\":[{\"id\":\"lead-1\"}]}"
                }
            }
        });
        assert_eq!(
            automation_connector_capture_reader_args_remote_paths(&prior_remote_file_read),
            vec!["/mnt/files/mex/seen.json".to_string()]
        );
        assert!(
            !automation_connector_capture_reader_args_mention_any_remote_path(
                &prior_remote_file_read,
                &remote_file_paths
            ),
            "the prior path is not the newly advertised file, but it is still an exact remote file read"
        );
        assert_eq!(
            automation_connector_capture_matched_remote_paths(
                &automation_connector_capture_reader_args_remote_paths(&prior_remote_file_read),
                &remote_file_paths,
            ),
            Vec::<String>::new(),
            "reading a different exact remote file must not satisfy the current connector result"
        );
        assert_eq!(
            automation_connector_capture_matched_remote_paths(
                &automation_connector_capture_reader_args_remote_paths(&remote_file_read),
                &remote_file_paths,
            ),
            vec!["/mnt/files/mex/pour.json".to_string()],
            "only the advertised remote result path satisfies hydration"
        );

        let remote_artifact_write = json!({
            "args": {
                "code_to_execute": "fallback = pathlib.Path('/mnt/files/search-shadow-ai-governance.json'); fallback.write_text(payload); glob('/mnt/files/**/*.json'); pathlib.Path('/mnt/files/.tandem/run/artifact.json').write_text(payload)"
            }
        });
        assert!(
            automation_connector_capture_reader_args_remote_paths(&remote_artifact_write)
                .is_empty(),
            "root-level /mnt/files artifacts written by the helper are not connector result hydration"
        );
    }

    #[test]
    fn connector_capture_validation_metadata_names_exact_remote_file_paths() {
        let mut validation = json!({});
        let capture = json!({
            "artifact_path": ".tandem/runs/run/artifacts/source-connector-results.json",
            "remote_hydration_required": true,
            "remote_file_paths": ["/mnt/files/mex/play.json"]
        });

        apply_automation_connector_capture_validation_metadata(
            &mut validation,
            &capture,
            1,
            3,
            false,
        );

        let actions = validation
            .get("required_next_tool_actions")
            .and_then(Value::as_array)
            .expect("required actions");
        let action_text = actions
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(action_text.contains("/mnt/files/mex/play.json"));
        assert!(action_text.contains("must reference these path(s) directly"));
        assert!(validation
            .get("connector_remote_file_paths")
            .and_then(Value::as_array)
            .is_some_and(|paths| paths
                .iter()
                .any(|path| path.as_str() == Some("/mnt/files/mex/play.json"))));
    }

    #[test]
    fn connector_capture_validation_requires_remote_file_read_even_with_materialized_source_artifact(
    ) {
        let mut validation = json!({});
        let capture = json!({
            "artifact_path": ".tandem/runs/run/artifacts/source-connector-results.json",
            "remote_hydration_required": true,
            "remote_file_paths": ["/mnt/files/mex/play.json"]
        });

        apply_automation_connector_capture_validation_metadata(
            &mut validation,
            &capture,
            3,
            3,
            true,
        );

        assert_eq!(
            validation
                .get("semantic_block_reason")
                .and_then(Value::as_str),
            Some("connector returned a remote result file, but the workflow artifact was finalized before materializing it")
        );
        assert!(validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|rows| rows.iter().any(|value| {
                value.as_str() == Some("connector_remote_result_not_materialized")
            })));
        assert!(validation
            .get("required_next_tool_actions")
            .and_then(Value::as_array)
            .is_some_and(|rows| rows.iter().any(|value| value
                .as_str()
                .is_some_and(|text| text.contains("/mnt/files/mex/play.json")))));
    }
}
