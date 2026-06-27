fn automation_connector_source_artifact_candidates(value: &Value, candidates: &mut Vec<Value>) {
    match value {
        Value::Object(object) => {
            if let Some(artifact) = object.get("artifact") {
                if automation_artifact_json_has_materialized_source_rows(artifact, 0) {
                    candidates.push(artifact.clone());
                }
            }
            if object.contains_key("status")
                && automation_artifact_json_has_materialized_source_rows(value, 0)
            {
                candidates.push(value.clone());
            }
            for child in object.values() {
                automation_connector_source_artifact_candidates(child, candidates);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                automation_connector_source_artifact_candidates(row, candidates);
            }
        }
        Value::String(text) => {
            if !(text.contains("\"artifact\"") || text.contains("\"raw_posts\"")) {
                return;
            }
            for (offset, _) in text.match_indices('{') {
                let Some(candidate) = text.get(offset..) else {
                    continue;
                };
                let parsed = serde_json::from_str::<Value>(candidate).ok().or_else(|| {
                    automation_balanced_json_object_slice(candidate)
                        .and_then(|slice| serde_json::from_str::<Value>(slice).ok())
                });
                if let Some(value) = parsed {
                    automation_connector_source_artifact_candidates(&value, candidates);
                }
            }
        }
        _ => {}
    }
}

fn automation_connector_source_artifact_candidate(value: &Value) -> Option<Value> {
    let mut candidates = Vec::new();
    automation_connector_source_artifact_candidates(value, &mut candidates);
    candidates
        .into_iter()
        .max_by_key(automation_connector_source_artifact_row_count)
}

fn automation_connector_source_artifact_needs_recovery(
    node: &AutomationFlowNode,
    artifact_text: &str,
) -> bool {
    let Ok(artifact) = serde_json::from_str::<Value>(artifact_text) else {
        return true;
    };
    if !automation_artifact_json_has_materialized_source_rows(&artifact, 0) {
        return true;
    }
    node.output_contract
        .as_ref()
        .and_then(|contract| contract.schema.as_ref())
        .and_then(|schema| automation_output_schema_validation_issue(schema, &artifact))
        .is_some()
}

fn automation_connector_source_row_looks_materialized(row: &serde_json::Map<String, Value>) -> bool {
    [
        "title",
        "url",
        "permalink",
        "author",
        "selftext",
        "body",
        "id",
        "created_utc",
        "score",
        "num_comments",
    ]
    .iter()
    .any(|key| row.contains_key(*key))
}

fn automation_collect_connector_source_rows_from_value(
    value: &Value,
    rows: &mut Vec<Value>,
    depth: usize,
) {
    if depth > 8 || rows.len() >= 500 {
        return;
    }
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if !automation_source_array_key(key) {
                    continue;
                }
                let Some(items) = child.as_array() else {
                    continue;
                };
                for item in items {
                    match item {
                        Value::Object(item_object)
                            if automation_connector_source_row_looks_materialized(item_object) =>
                        {
                            rows.push(item.clone());
                        }
                        Value::Object(_) | Value::Array(_) => {
                            automation_collect_connector_source_rows_from_value(
                                item,
                                rows,
                                depth + 1,
                            );
                        }
                        _ => {}
                    }
                    if rows.len() >= 500 {
                        return;
                    }
                }
            }
            for child in object.values() {
                automation_collect_connector_source_rows_from_value(child, rows, depth + 1);
                if rows.len() >= 500 {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                automation_collect_connector_source_rows_from_value(item, rows, depth + 1);
                if rows.len() >= 500 {
                    return;
                }
            }
        }
        Value::String(text) => {
            if text.contains("\"artifact\"") {
                return;
            }
            if !(text.contains("\"posts\"")
                || text.contains("\"raw_posts\"")
                || text.contains("\"items\"")
                || text.contains("\"records\""))
            {
                return;
            }
            for (offset, _) in text.match_indices('{') {
                let Some(candidate) = text.get(offset..) else {
                    continue;
                };
                let Ok(parsed) = serde_json::from_str::<Value>(candidate) else {
                    continue;
                };
                automation_collect_connector_source_rows_from_value(&parsed, rows, depth + 1);
                if rows.len() >= 500 {
                    return;
                }
            }
        }
        _ => {}
    }
}

fn automation_connector_source_row_key(row: &Value) -> String {
    row.get("permalink")
        .or_else(|| row.get("url"))
        .or_else(|| row.get("id"))
        .or_else(|| row.get("title"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| row.to_string())
}

fn automation_connector_source_artifact_row_count(value: &Value) -> usize {
    let mut rows = Vec::new();
    automation_collect_connector_source_rows_from_value(value, &mut rows, 0);
    rows.len()
}

fn automation_node_connector_source_array_key(node: &AutomationFlowNode) -> String {
    let Some(schema) = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.schema.as_ref())
    else {
        return "source_items".to_string();
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        if let Some(key) = required
            .iter()
            .filter_map(Value::as_str)
            .find(|key| automation_source_array_key(key))
        {
            return key.to_string();
        }
    }
    schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| {
            properties
                .iter()
                .find(|(key, property)| {
                    automation_source_array_key(key)
                        && property.get("type").and_then(Value::as_str) == Some("array")
                })
                .map(|(key, _)| key.clone())
        })
        .unwrap_or_else(|| "source_items".to_string())
}

fn automation_node_metadata_string(node: &AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| automation_metadata_string_value(metadata, key))
        .map(str::to_string)
}

fn automation_node_metadata_string_array(node: &AutomationFlowNode, key: &str) -> Vec<String> {
    match node.metadata.as_ref().and_then(|metadata| metadata.get(key)) {
        Some(Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn automation_build_connector_source_artifact_from_raw_payload(
    node: &AutomationFlowNode,
    value: &Value,
) -> Option<Value> {
    let mut rows = Vec::new();
    automation_collect_connector_source_rows_from_value(value, &mut rows, 0);
    if rows.is_empty() {
        return None;
    }
    let source_query = automation_node_metadata_string(node, "source_query")
        .or_else(|| automation_node_metadata_string(node, "query"));
    let mut seen = std::collections::BTreeSet::new();
    let rows = rows
        .into_iter()
        .filter_map(|mut row| {
            if let (Some(query), Some(object)) = (source_query.as_ref(), row.as_object_mut()) {
                object
                    .entry("source_query".to_string())
                    .or_insert_with(|| json!(query));
            }
            let key = automation_connector_source_row_key(&row);
            seen.insert(key).then_some(row)
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return None;
    }
    let mut artifact = serde_json::Map::new();
    artifact.insert("status".to_string(), json!("completed"));
    if let Some(query) = source_query {
        artifact.insert("query".to_string(), json!(query));
    }
    if let Some(search_query) = automation_node_metadata_string(node, "search_query") {
        artifact.insert("search_query".to_string(), json!(search_query));
    }
    artifact.insert(
        automation_node_connector_source_array_key(node),
        Value::Array(rows),
    );
    artifact.insert("limitations".to_string(), Value::Array(Vec::new()));
    Some(Value::Object(artifact))
}

fn automation_parse_connector_source_artifact_from_text(
    node: &AutomationFlowNode,
    text: &str,
) -> Option<Value> {
    let mut candidates = Vec::new();
    for (offset, _) in text.match_indices('{') {
        let candidate = text.get(offset..)?;
        let parsed = serde_json::from_str::<Value>(candidate).ok().or_else(|| {
            automation_balanced_json_object_slice(candidate)
                .and_then(|slice| serde_json::from_str::<Value>(slice).ok())
        });
        if let Some(value) = parsed {
            if let Some(artifact) = automation_connector_source_artifact_candidate(&value) {
                candidates.push(artifact);
                continue;
            }
            if let Some(artifact) =
                automation_build_connector_source_artifact_from_raw_payload(node, &value)
            {
                candidates.push(artifact);
            }
        }
    }
    candidates
        .into_iter()
        .max_by_key(automation_connector_source_artifact_row_count)
}

fn automation_balanced_json_object_slice(text: &str) -> Option<&str> {
    if !text.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_string {
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return text.get(..=index);
                }
            }
            _ => {}
        }
    }
    None
}

fn automation_collect_connector_result_texts(value: &Value, texts: &mut Vec<String>, depth: usize) {
    if depth > 6 || texts.len() >= 64 {
        return;
    }
    match value {
        Value::String(text) => {
            if text.contains("\"raw_posts\"")
                || text.contains("\"artifact\"")
                || text.contains("\"posts\"")
            {
                texts.push(text.clone());
            }
        }
        Value::Array(rows) => {
            for row in rows {
                automation_collect_connector_result_texts(row, texts, depth + 1);
                if texts.len() >= 64 {
                    return;
                }
            }
        }
        Value::Object(object) => {
            for key in ["stdout", "results", "output", "artifact", "data", "response"] {
                if let Some(child) = object.get(key) {
                    automation_collect_connector_result_texts(child, texts, depth + 1);
                    if texts.len() >= 64 {
                        return;
                    }
                }
            }
            for child in object.values() {
                automation_collect_connector_result_texts(child, texts, depth + 1);
                if texts.len() >= 64 {
                    return;
                }
            }
        }
        _ => {}
    }
}

fn automation_connector_result_text_rank(text: &str) -> u8 {
    if text.contains("\"artifact\"") && text.contains("\"raw_posts\"") {
        0
    } else if text.contains("\"artifact\"") {
        1
    } else if text.contains("\"raw_posts\"") {
        2
    } else if text.contains("\"posts\"") {
        3
    } else {
        4
    }
}

fn automation_connector_source_artifact_from_texts(
    node: &AutomationFlowNode,
    texts: &mut [String],
) -> Option<Value> {
    texts.sort_by_key(|text| automation_connector_result_text_rank(text));
    texts
        .iter()
        .find_map(|text| automation_parse_connector_source_artifact_from_text(node, text))
}

fn automation_connector_source_artifact_from_value(
    node: &AutomationFlowNode,
    value: &Value,
) -> Option<Value> {
    let mut candidates = Vec::new();
    if let Some(artifact) = automation_connector_source_artifact_candidate(value) {
        candidates.push(artifact);
    }
    if let Some(artifact) = automation_build_connector_source_artifact_from_raw_payload(node, value)
    {
        candidates.push(artifact);
    }
    candidates
        .into_iter()
        .max_by_key(automation_connector_source_artifact_row_count)
}

fn automation_write_recovered_connector_source_artifact(
    artifact: Value,
    workspace_root: &str,
    output_path: &str,
) -> anyhow::Result<(String, String, AutomationVerifiedOutputResolution)> {
    let serialized = serde_json::to_string_pretty(&artifact)?;
    let resolved = resolve_automation_output_path(workspace_root, output_path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&resolved, &serialized)?;
    let display_path = resolved
        .strip_prefix(workspace_root)
        .ok()
        .and_then(|value| value.to_str().map(str::to_string))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| output_path.to_string());
    Ok((
        display_path,
        serialized,
        AutomationVerifiedOutputResolution {
            path: resolved,
            legacy_workspace_artifact_promoted_from: None,
            materialized_by_current_attempt: true,
            resolution_kind: AutomationVerifiedOutputResolutionKind::SessionTextRecovery,
        },
    ))
}

fn automation_recover_connector_source_artifact_from_session(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    _run_id: &str,
    output_path: &str,
) -> anyhow::Result<Option<(String, String, AutomationVerifiedOutputResolution)>> {
    if !automation_node_expects_connector_source_materialization(node) {
        return Ok(None);
    }

    let mut texts = Vec::new();
    let mut candidates = Vec::new();
    for part in session.messages.iter().flat_map(|message| message.parts.iter()) {
        let MessagePart::ToolInvocation { result, .. } = part else {
            continue;
        };
        let payload = automation_connector_capture_result_payload(result.as_ref());
        if let Some(artifact) = automation_connector_source_artifact_from_value(node, &payload) {
            candidates.push(artifact);
        }
        automation_collect_connector_result_texts(&payload, &mut texts, 0);
    }

    if let Some(artifact) = automation_connector_source_artifact_from_texts(node, &mut texts) {
        candidates.push(artifact);
    }

    if let Some(artifact) = candidates
        .into_iter()
        .max_by_key(automation_connector_source_artifact_row_count)
    {
        return Ok(Some(automation_write_recovered_connector_source_artifact(
            artifact,
            workspace_root,
            output_path,
        )?));
    }

    Ok(None)
}

fn automation_recover_connector_source_artifact_from_capture(
    node: &AutomationFlowNode,
    workspace_root: &str,
    output_path: &str,
    connector_capture: &Value,
) -> anyhow::Result<Option<(String, String, AutomationVerifiedOutputResolution)>> {
    if !automation_node_expects_connector_source_materialization(node) {
        return Ok(None);
    }
    let Some(path) = connector_capture
        .get("artifact_path")
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let resolved = resolve_automation_output_path(workspace_root, path)?;
    let Ok(text) = std::fs::read_to_string(resolved) else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Ok(None);
    };
    let mut texts = Vec::new();
    automation_collect_connector_result_texts(&value, &mut texts, 0);
    if let Some(artifact) = automation_connector_source_artifact_from_texts(node, &mut texts) {
        return Ok(Some(automation_write_recovered_connector_source_artifact(
            artifact,
            workspace_root,
            output_path,
        )?));
    }
    Ok(None)
}

fn automation_connector_capture_string_array(capture: &Value, key: &str) -> Vec<String> {
    capture
        .get(key)
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_connector_remote_python_helper_tool(
    requested_tools: &[String],
    connector_capture: &Value,
) -> Option<String> {
    let capture_tools = automation_connector_capture_string_array(connector_capture, "tools");
    let mut candidates = requested_tools
        .iter()
        .chain(capture_tools.iter())
        .filter_map(|tool| {
            if tool.ends_with(".composio_remote_workbench") {
                Some((0u8, tool.clone()))
            } else if tool.ends_with(".composio_remote_bash_tool") {
                Some((1u8, tool.clone()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    candidates.dedup_by(|left, right| left.1 == right.1);
    candidates.into_iter().map(|(_, tool)| tool).next()
}

pub(crate) fn automation_connector_remote_python_args(
    helper_tool: &str,
    code: String,
    current_step: &str,
    session_id: &str,
    workspace_root: &str,
) -> Value {
    let mut args = if helper_tool.ends_with(".composio_remote_bash_tool") {
        json!({
            "command": format!("python3 - <<'PY'\n{code}\nPY"),
        })
    } else {
        json!({
            "code_to_execute": code,
        })
    };
    if let Some(object) = args.as_object_mut() {
        object.insert("current_step".to_string(), json!(current_step));
        object.insert("current_step_metric".to_string(), json!("1/1"));
        object.insert("session_id".to_string(), json!(session_id));
        object.insert("__session_id".to_string(), json!(session_id));
        object.insert("__workspace_root".to_string(), json!(workspace_root));
        object.insert("__effective_cwd".to_string(), json!(workspace_root));
        object.insert(
            "__project_id".to_string(),
            json!(automation_workspace_project_id(workspace_root)),
        );
    }
    args
}
fn automation_connector_existing_artifact_preserved_fields(
    node: &AutomationFlowNode,
    workspace_root: &str,
    output_path: &str,
) -> Value {
    let mut preserved = serde_json::Map::new();
    if let Ok(resolved) = resolve_automation_output_path(workspace_root, output_path) {
        if let Ok(text) = std::fs::read_to_string(resolved) {
            if let Ok(Value::Object(existing)) = serde_json::from_str::<Value>(&text) {
                for key in ["query", "search_query", "source_query"] {
                    if let Some(value) = existing.get(key) {
                        if !value.is_null() {
                            preserved.insert(key.to_string(), value.clone());
                        }
                    }
                }
            }
        }
    }
    for key in ["query", "source_query", "search_query"] {
        if preserved.contains_key(key) {
            continue;
        }
        if let Some(value) = automation_node_metadata_string(node, key) {
            let output_key = if key == "source_query" { "query" } else { key };
            preserved
                .entry(output_key.to_string())
                .or_insert_with(|| json!(value));
        }
    }
    Value::Object(preserved)
}

fn automation_connector_remote_materializer_code(
    remote_paths: &[String],
    output_path: &std::path::Path,
    array_key: &str,
    preserved_fields: &Value,
) -> anyhow::Result<String> {
    let remote_paths = serde_json::to_string(remote_paths)?;
    let output_path = output_path.to_string_lossy().to_string();
    let preserved_fields = serde_json::to_string(preserved_fields)?;
    let array_key = serde_json::to_string(array_key)?;
    Ok(format!(
        r#"
import json
import re
from pathlib import Path

remote_paths = json.loads(r'''{remote_paths}''')
out_path = Path(r'''{output_path}''')
array_key = json.loads(r'''{array_key}''')
preserved = json.loads(r'''{preserved_fields}''')
source_keys = {{
    "raw_posts", "posts", "raw_results", "source_items", "items", "records",
    "rows", "threads", "leads", "results", "documents", "entries", "hits", "values"
}}
row_keys = {{
    "title", "url", "permalink", "author", "selftext", "body", "id",
    "created_utc", "score", "num_comments"
}}
rows = []

def row_looks_materialized(value):
    return isinstance(value, dict) and any(key in value for key in row_keys)

def normalize_row(value):
    if not isinstance(value, dict):
        return None
    if isinstance(value.get("data"), dict) and row_looks_materialized(value["data"]):
        value = dict(value["data"])
    if not row_looks_materialized(value):
        return None
    row = dict(value)
    permalink = row.get("permalink")
    if isinstance(permalink, str) and permalink.startswith("/r/"):
        row["permalink"] = "https://www.reddit.com" + permalink
        row.setdefault("url", row["permalink"])
    return row

def collect(value, depth=0):
    if depth > 8 or len(rows) >= 500:
        return
    if isinstance(value, dict):
        for key, child in value.items():
            if key not in source_keys or not isinstance(child, list):
                continue
            for item in child:
                normalized = normalize_row(item)
                if normalized is not None:
                    rows.append(normalized)
                elif isinstance(item, (dict, list)):
                    collect(item, depth + 1)
                if len(rows) >= 500:
                    return
        for child in value.values():
            if isinstance(child, (dict, list, str)):
                collect(child, depth + 1)
            if len(rows) >= 500:
                return
    elif isinstance(value, list):
        for item in value:
            collect(item, depth + 1)
            if len(rows) >= 500:
                return
    elif isinstance(value, str) and any(marker in value for marker in ('"posts"', '"raw_posts"', '"items"', '"records"')):
        for offset in [m.start() for m in re.finditer(r'[\{{\[]', value)]:
            try:
                collect(json.loads(value[offset:]), depth + 1)
            except Exception:
                pass
            if len(rows) >= 500:
                return

loaded_paths = []
for remote_path in remote_paths:
    with open(remote_path, "r", encoding="utf-8") as handle:
        payload = json.load(handle)
    loaded_paths.append(remote_path)
    collect(payload)

deduped = []
seen = set()
for row in rows:
    key = row.get("permalink") or row.get("url") or row.get("id") or row.get("title") or json.dumps(row, sort_keys=True)
    if key in seen:
        continue
    seen.add(key)
    if preserved.get("query") and isinstance(row, dict):
        row.setdefault("source_query", preserved["query"])
    deduped.append(row)

artifact = {{"status": "completed"}}
for key, value in preserved.items():
    if value is not None:
        artifact[key] = value
artifact[array_key] = deduped
artifact["limitations"] = [] if deduped else [
    "Connector remote result file was materialized, but no source rows matching the node contract were found."
]
artifact["connector_materialization"] = {{
    "source": "deterministic_remote_result_materializer",
    "remote_file_paths": loaded_paths,
    "row_count": len(deduped),
}}
out_path.parent.mkdir(parents=True, exist_ok=True)
out_path.write_text(json.dumps(artifact, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
remote_artifact_dir = Path("/mnt/files/.composio/output")
remote_artifact_dir.mkdir(parents=True, exist_ok=True)
remote_artifact_path = remote_artifact_dir / ("tandem_connector_artifact_" + __import__("hashlib").sha256(str(out_path).encode("utf-8")).hexdigest()[:16] + ".json")
remote_artifact_path.write_text(json.dumps(artifact, ensure_ascii=False), encoding="utf-8")
print(json.dumps({{
    "status": "completed",
    "output_path": str(out_path),
    "remote_artifact_path": str(remote_artifact_path),
    "row_count": len(deduped),
    "remote_file_paths": loaded_paths,
}}, ensure_ascii=False))
"#
    ))
}

fn automation_connector_remote_file_chunk_reader_code(
    remote_artifact_path: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<String> {
    let remote_artifact_path = serde_json::to_string(remote_artifact_path)?;
    Ok(format!(
        r#"
import base64
import json
from pathlib import Path

remote_artifact_path = json.loads(r'''{remote_artifact_path}''')
offset = {offset}
limit = {limit}
payload = Path(remote_artifact_path).read_bytes()
chunk = payload[offset:offset + limit]
print(json.dumps({{
    "status": "chunk",
    "remote_artifact_path": remote_artifact_path,
    "offset": offset,
    "total": len(payload),
    "data_b64": base64.b64encode(chunk).decode("ascii"),
}}, ensure_ascii=False))
"#
    ))
}

fn automation_collect_connector_materializer_texts(value: &Value, texts: &mut Vec<String>, depth: usize) {
    if depth > 8 || texts.len() >= 128 {
        return;
    }
    match value {
        Value::String(text) => {
            if text.contains("remote_artifact_path")
                || text.contains("\"data_b64\"")
                || text.contains("\"stdout\"")
                || text.contains("\"artifact\"")
            {
                texts.push(text.clone());
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                automation_collect_connector_materializer_texts(&parsed, texts, depth + 1);
            } else if let Some(slice) = automation_balanced_json_object_slice(text) {
                if let Ok(parsed) = serde_json::from_str::<Value>(slice) {
                    automation_collect_connector_materializer_texts(&parsed, texts, depth + 1);
                }
            }
        }
        Value::Array(rows) => {
            for row in rows {
                automation_collect_connector_materializer_texts(row, texts, depth + 1);
                if texts.len() >= 128 {
                    return;
                }
            }
        }
        Value::Object(object) => {
            for key in [
                "stdout",
                "text",
                "output",
                "content",
                "result",
                "data",
                "metadata",
                "response",
            ] {
                if let Some(child) = object.get(key) {
                    automation_collect_connector_materializer_texts(child, texts, depth + 1);
                    if texts.len() >= 128 {
                        return;
                    }
                }
            }
            for child in object.values() {
                automation_collect_connector_materializer_texts(child, texts, depth + 1);
                if texts.len() >= 128 {
                    return;
                }
            }
        }
        _ => {}
    }
}

fn automation_connector_materializer_json_objects(value: &Value) -> Vec<Value> {
    let mut texts = Vec::new();
    automation_collect_connector_materializer_texts(value, &mut texts, 0);
    let mut objects = Vec::new();
    for text in texts {
        for (offset, _) in text.match_indices('{') {
            let Some(candidate) = text.get(offset..) else {
                continue;
            };
            let parsed = serde_json::from_str::<Value>(candidate).ok().or_else(|| {
                automation_balanced_json_object_slice(candidate)
                    .and_then(|slice| serde_json::from_str::<Value>(slice).ok())
            });
            if let Some(value) = parsed {
                objects.push(value);
            }
        }
    }
    objects
}

fn automation_connector_materializer_descriptor(value: &Value) -> Option<Value> {
    automation_connector_materializer_json_objects(value)
        .into_iter()
        .find(|value| {
            value
                .get("remote_artifact_path")
                .and_then(Value::as_str)
                .is_some()
                && value.get("status").and_then(Value::as_str) == Some("completed")
        })
}

fn automation_connector_materializer_chunk(value: &Value) -> Option<Value> {
    automation_connector_materializer_json_objects(value)
        .into_iter()
        .find(|value| {
            value.get("data_b64").and_then(Value::as_str).is_some()
                && value.get("offset").and_then(Value::as_u64).is_some()
                && value.get("total").and_then(Value::as_u64).is_some()
        })
}

#[allow(clippy::too_many_arguments)]
async fn automation_fetch_connector_remote_artifact(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    session: &mut Session,
    workspace_root: &str,
    requested_tools: &[String],
    helper_tool: &str,
    remote_artifact_path: &str,
) -> anyhow::Result<Option<Value>> {
    const CHUNK_SIZE: usize = 12_000;
    const MAX_REMOTE_ARTIFACT_BYTES: usize = 10 * 1024 * 1024;
    const MAX_CHUNKS: usize = 1024;

    let mut offset = 0usize;
    let mut expected_total = None;
    let mut bytes = Vec::new();
    for _ in 0..MAX_CHUNKS {
        let code = automation_connector_remote_file_chunk_reader_code(
            remote_artifact_path,
            offset,
            CHUNK_SIZE,
        )?;
        let mut args = automation_connector_remote_python_args(
            helper_tool,
            code,
            "FETCH_CONNECTOR_REMOTE_RESULT_CHUNK",
            &session.id,
            workspace_root,
        );
        if let Some(object) = args.as_object_mut() {
            object.insert(
                "remote_artifact_path".to_string(),
                json!(remote_artifact_path),
            );
            object.insert("offset".to_string(), json!(offset));
            object.insert("limit".to_string(), json!(CHUNK_SIZE));
        }
        let tenant_context = automation.tenant_context();
        let dispatch_context = state.tool_dispatch_context(
            tandem_tools::ToolDispatchSource::new("automation_connector_remote_artifact_fetch")
                .run(run_id)
                .node(node.node_id.clone()),
            tenant_context,
            requested_tools.to_vec(),
        );
        let result = state
            .tool_dispatcher
            .dispatch(helper_tool, args.clone(), dispatch_context)
            .await?;
        let result_value = json!({
            "output": result.output,
            "metadata": result.metadata,
        });
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: helper_tool.to_string(),
                args,
                result: Some(result_value.clone()),
                error: None,
            }],
        ));
        let Some(chunk) = automation_connector_materializer_chunk(&result_value) else {
            return Ok(None);
        };
        let total = chunk
            .get("total")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        if total == 0 || total > MAX_REMOTE_ARTIFACT_BYTES {
            return Ok(None);
        }
        let chunk_offset = chunk
            .get("offset")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(usize::MAX);
        if chunk_offset != offset {
            return Ok(None);
        }
        let Some(encoded) = chunk.get("data_b64").and_then(Value::as_str) else {
            return Ok(None);
        };
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD.decode(encoded.trim())?;
        bytes.extend(decoded);
        expected_total = Some(total);
        offset = bytes.len();
        if offset >= total {
            break;
        }
    }
    if expected_total != Some(bytes.len()) {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice::<Value>(&bytes)?))
}

fn automation_connector_capture_mark_materialized(
    connector_capture: &Value,
    workspace_root: &str,
    remote_paths: &[String],
    helper_tool: &str,
    materializer_result: &Value,
    hydration_succeeded: bool,
) -> anyhow::Result<Value> {
    let mut updated = connector_capture.clone();
    if let Some(object) = updated.as_object_mut() {
        object.insert(
            "remote_hydration_required".to_string(),
            json!(!hydration_succeeded),
        );
        object.insert(
            "remote_hydration_performed".to_string(),
            json!(hydration_succeeded),
        );
        if hydration_succeeded {
            object.insert("remote_reader_matched_paths".to_string(), json!(remote_paths));
        }
        object.insert(
            "deterministic_remote_materialization".to_string(),
            json!({
                "attempted": true,
                "performed": hydration_succeeded,
                "tool": helper_tool,
                "result": materializer_result,
            }),
        );
    }

    if let Some(path) = connector_capture
        .get("artifact_path")
        .and_then(Value::as_str)
    {
        let resolved = resolve_automation_output_path(workspace_root, path)?;
        if let Ok(text) = std::fs::read_to_string(&resolved) {
            if let Ok(mut sidecar) = serde_json::from_str::<Value>(&text) {
                if let Some(object) = sidecar.as_object_mut() {
                    object.insert(
                        "remote_hydration_required".to_string(),
                        json!(!hydration_succeeded),
                    );
                    object.insert(
                        "remote_hydration_performed".to_string(),
                        json!(hydration_succeeded),
                    );
                    if hydration_succeeded {
                        object.insert("remote_reader_matched_paths".to_string(), json!(remote_paths));
                    }
                    object.insert(
                        "deterministic_remote_materialization".to_string(),
                        json!({
                            "attempted": true,
                            "performed": hydration_succeeded,
                            "tool": helper_tool,
                            "result": materializer_result,
                        }),
                    );
                }
                std::fs::write(&resolved, serde_json::to_string_pretty(&sidecar)?)?;
            }
        }
    }
    Ok(updated)
}

#[allow(clippy::too_many_arguments)]
async fn automation_try_materialize_connector_remote_result_artifact(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    session: &mut Session,
    workspace_root: &str,
    output_path: &str,
    requested_tools: &[String],
    connector_capture: &Value,
) -> anyhow::Result<Option<(
    (String, String, AutomationVerifiedOutputResolution),
    Value,
)>> {
    if !automation_node_expects_connector_source_materialization(node) {
        return Ok(None);
    }
    let remote_paths =
        automation_connector_capture_string_array(connector_capture, "remote_file_paths");
    if remote_paths.is_empty() {
        return Ok(None);
    }
    let Some(helper_tool) =
        automation_connector_remote_python_helper_tool(requested_tools, connector_capture)
    else {
        return Ok(None);
    };

    let resolved_output = resolve_automation_output_path(workspace_root, output_path)?;
    let preserved_fields =
        automation_connector_existing_artifact_preserved_fields(node, workspace_root, output_path);
    let code = automation_connector_remote_materializer_code(
        &remote_paths,
        &resolved_output,
        &automation_node_connector_source_array_key(node),
        &preserved_fields,
    )?;
    let mut args = automation_connector_remote_python_args(
        &helper_tool,
        code,
        "MATERIALIZE_CONNECTOR_REMOTE_RESULT",
        &session.id,
        workspace_root,
    );
    if let Some(object) = args.as_object_mut() {
        object.insert(
            "remote_file_paths".to_string(),
            json!(remote_paths.clone()),
        );
    }
    let tenant_context = automation.tenant_context();
    let dispatch_context = state.tool_dispatch_context(
        tandem_tools::ToolDispatchSource::new("automation_connector_remote_materializer")
            .run(run_id)
            .node(node.node_id.clone()),
        tenant_context,
        requested_tools.to_vec(),
    );
    let result = state
        .tool_dispatcher
        .dispatch(&helper_tool, args.clone(), dispatch_context)
        .await?;
    let result_value = json!({
        "output": result.output,
        "metadata": result.metadata,
    });
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: helper_tool.clone(),
            args: args.clone(),
            result: Some(result_value.clone()),
            error: None,
        }],
    ));

    let descriptor = automation_connector_materializer_descriptor(&result_value);
    let remote_artifact = descriptor
        .as_ref()
        .and_then(|value| value.get("remote_artifact_path"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut remote_hydration_succeeded = false;
    let mut recovered = None;
    if let Some(remote_artifact_path) = remote_artifact.as_deref() {
        recovered = automation_fetch_connector_remote_artifact(
            state,
            run_id,
            automation,
            node,
            session,
            workspace_root,
            requested_tools,
            &helper_tool,
            remote_artifact_path,
        )
        .await?
        .map(|artifact| {
            automation_write_recovered_connector_source_artifact(
                artifact,
                workspace_root,
                output_path,
            )
        })
        .transpose()?;
        remote_hydration_succeeded = recovered.is_some();
    }
    if recovered.is_none() {
        let mut texts = Vec::new();
        let mut candidates = Vec::new();
        if let Some(artifact) = automation_connector_source_artifact_from_value(node, &result_value)
        {
            candidates.push(artifact);
        }
        automation_collect_connector_result_texts(&result_value, &mut texts, 0);
        if let Some(artifact) = automation_connector_source_artifact_from_texts(node, &mut texts) {
            candidates.push(artifact);
        }
        recovered = candidates
            .into_iter()
            .max_by_key(automation_connector_source_artifact_row_count)
            .map(|artifact| {
                automation_write_recovered_connector_source_artifact(
                    artifact,
                    workspace_root,
                    output_path,
                )
            })
            .transpose()?;
    }
    state.storage.save_session(session.clone()).await?;

    let materialized = if let Some(recovered) = recovered {
        recovered
    } else {
        if !resolved_output.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&resolved_output)?;
        let display_path = resolved_output
            .strip_prefix(workspace_root)
            .ok()
            .and_then(|value| value.to_str().map(str::to_string))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| output_path.to_string());
        (
            display_path,
            text,
            AutomationVerifiedOutputResolution {
                path: resolved_output.clone(),
                legacy_workspace_artifact_promoted_from: None,
                materialized_by_current_attempt: true,
                resolution_kind: AutomationVerifiedOutputResolutionKind::SessionTextRecovery,
            },
        )
    };
    let updated_capture = automation_connector_capture_mark_materialized(
        connector_capture,
        workspace_root,
        &remote_paths,
        &helper_tool,
        &json!({
            "descriptor": descriptor,
            "fetched_remote_artifact_path": remote_artifact,
            "result": result_value,
        }),
        remote_hydration_succeeded,
    )?;
    Ok(Some((
        materialized,
        updated_capture,
    )))
}

async fn automation_resolve_required_output_candidate(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
) -> anyhow::Result<(
    Option<(String, String, AutomationVerifiedOutputResolution)>,
    Option<String>,
)> {
    match reconcile_automation_resolve_verified_output_path(
        session,
        workspace_root,
        run_id,
        node,
        output_path,
        250,
        25,
    )
    .await
    {
        Ok(Some(resolution)) => {
            let resolved = resolution.path.clone();
            if !resolved.is_file() {
                return Ok((
                    None,
                    Some(format!(
                        "required output `{}` for node `{}` is not a file",
                        output_path, node.node_id
                    )),
                ));
            }
            match std::fs::read_to_string(&resolved) {
                Ok(file_text) => {
                    let display_path = resolved
                        .strip_prefix(workspace_root)
                        .ok()
                        .and_then(|value| value.to_str().map(str::to_string))
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| output_path.to_string());
                    Ok((Some((display_path, file_text, resolution)), None))
                }
                Err(error) => Ok((
                    None,
                    Some(format!(
                        "required output `{}` for node `{}` could not be read: {}",
                        output_path, node.node_id, error
                    )),
                )),
            }
        }
        Ok(None) => Ok((
            None,
            Some(format!(
                "required output `{}` was not created for node `{}`",
                output_path, node.node_id
            )),
        )),
        Err(error) => Ok((
            None,
            Some(format!(
                "required output `{}` for node `{}` could not be reconciled: {}",
                output_path, node.node_id, error
            )),
        )),
    }
}

fn automation_should_materialize_connector_remote_result(
    node: &AutomationFlowNode,
    connector_capture: &Value,
    verified_output: Option<&(String, String, AutomationVerifiedOutputResolution)>,
) -> bool {
    let remote_hydration_required = connector_capture
        .get("remote_hydration_required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !remote_hydration_required {
        return false;
    }
    verified_output
        .map(|(_, text, _)| automation_connector_source_artifact_needs_recovery(node, text))
        .unwrap_or(true)
}

#[allow(clippy::too_many_arguments)]
async fn automation_materialize_connector_verified_output(
    state: &AppState,
    run: &AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    session: &mut Session,
    workspace_root: &str,
    output_path: &str,
    requested_tools: &[String],
    connector_capture: &mut Option<Value>,
    verified_output: &mut Option<(String, String, AutomationVerifiedOutputResolution)>,
    verified_output_error: &mut Option<String>,
) -> anyhow::Result<()> {
    if let Some(capture) = connector_capture.as_ref() {
        if automation_should_materialize_connector_remote_result(
            node,
            capture,
            verified_output.as_ref(),
        ) {
            if let Some((materialized, updated_capture)) =
                automation_try_materialize_connector_remote_result_artifact(
                state,
                run_id,
                automation,
                node,
                session,
                workspace_root,
                output_path,
                requested_tools,
                capture,
            )
            .await?
            {
                *verified_output = Some(materialized);
                *verified_output_error = None;
                *connector_capture = Some(updated_capture);
            }
        }
    }

    let should_try_connector_recovery = verified_output
        .as_ref()
        .map(|(_, text, _)| {
            connector_capture.is_some()
                && automation_node_expects_connector_source_materialization(node)
                && automation_connector_source_artifact_needs_recovery(node, text)
        })
        .unwrap_or(true);
    if should_try_connector_recovery {
        let recovered = automation_recover_connector_source_artifact_from_session(
            node,
            session,
            workspace_root,
            run_id,
            output_path,
        )?
        .or_else(|| {
            connector_capture.as_ref().and_then(|capture| {
                automation_recover_connector_source_artifact_from_capture(
                    node,
                    workspace_root,
                    output_path,
                    capture,
                )
                .ok()
                .flatten()
            })
        });
        if let Some(recovered) = recovered {
            *verified_output = Some(recovered);
            *verified_output_error = None;
        }
    }

    if let Some(materialized) = automation_try_materialize_connector_row_filter_artifact(
        run,
        node,
        workspace_root,
        output_path,
        verified_output.as_ref(),
    )? {
        *verified_output = Some(materialized);
        *verified_output_error = None;
    }
    Ok(())
}

fn automation_connector_source_artifact_stats(
    node: &AutomationFlowNode,
    verified_output_for_evidence: Option<&(String, String)>,
) -> (bool, usize) {
    if !automation_node_expects_connector_source_materialization(node) {
        return (false, 0);
    }
    let artifact = verified_output_for_evidence
        .and_then(|(_, text)| serde_json::from_str::<Value>(text).ok());
    let materialized = artifact
        .as_ref()
        .is_some_and(|artifact| automation_artifact_json_has_materialized_source_rows(artifact, 0));
    let row_count = artifact
        .as_ref()
        .map(automation_connector_source_artifact_row_count)
        .unwrap_or(0);
    (materialized, row_count)
}

fn apply_connector_truncated_preview_validation(
    artifact_validation: &mut Value,
    capture: &Value,
    attempt: u32,
    max_attempts: u32,
    connector_source_artifact_row_count: usize,
) {
    let preview_truncated = capture
        .get("extracted_items_truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let preview_count = capture
        .get("extracted_item_count")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    if !preview_truncated
        || connector_source_artifact_row_count == 0
        || connector_source_artifact_row_count > preview_count
    {
        return;
    }
    let Some(object) = artifact_validation.as_object_mut() else {
        return;
    };
    let unmet = object
        .entry("unmet_requirements".to_string())
        .or_insert_with(|| json!([]));
    if let Some(rows) = unmet.as_array_mut() {
        if !rows
            .iter()
            .any(|value| value.as_str() == Some("connector_truncated_preview_only"))
        {
            rows.push(json!("connector_truncated_preview_only"));
        }
    }
    object.insert(
        "semantic_block_reason".to_string(),
        json!("connector source artifact only materialized the truncated preview rows"),
    );
    object.insert(
        "rejected_artifact_reason".to_string(),
        json!("connector source artifact only materialized the truncated preview rows"),
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
        "connector_source_artifact_row_count".to_string(),
        json!(connector_source_artifact_row_count),
    );
    object.insert("connector_preview_row_count".to_string(), json!(preview_count));
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
    let actions = object
        .entry("required_next_tool_actions".to_string())
        .or_insert_with(|| json!([]));
    if let Some(rows) = actions.as_array_mut() {
        let action = if remote_file_paths.is_empty() {
            "Open the exact connector remote result file and rebuild the artifact from the full result, not the truncated data_preview rows.".to_string()
        } else {
            format!(
                "Open the exact connector remote result file(s) {} and rebuild the artifact from the full result, not the truncated data_preview rows.",
                remote_file_paths.join(", ")
            )
        };
        if !rows.iter().any(|value| value.as_str() == Some(action.as_str())) {
            rows.push(json!(action));
        }
    }
}

#[cfg(test)]
mod connector_source_artifact_recovery_tests {
    use super::*;

    fn source_node() -> AutomationFlowNode {
        AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "search_source".to_string(),
            agent_id: "agent".to_string(),
            objective: "Search a connector source and write rows.".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(AutomationFlowOutputContract {
                kind: "json_object".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                enforcement: None,
                schema: Some(json!({
                    "type": "object",
                    "required": ["raw_posts"],
                    "properties": {
                        "status": {"type": "string"},
                        "query": {"type": "string"},
                        "search_query": {"type": "string"},
                        "raw_posts": {"type": "array"}
                    }
                })),
                summary_guidance: None,
            }),
            tool_policy: None,
            mcp_policy: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "source_query": "local llm data privacy corporate",
                "search_query": "(local llm data privacy corporate) AND subreddit:LocalLLaMA"
            })),
        }
    }

    #[test]
    fn parses_connector_source_artifact_from_remote_stdout() {
        let node = source_node();
        let stdout = r#"READ_PATH /mnt/files/mex/roof.json
EXISTS True
{"count":1,"artifact":{"status":"completed","raw_posts":[{"title":"Lead","url":"https://example.test"}]}}
VERIFY complete"#;

        let artifact = automation_parse_connector_source_artifact_from_text(&node, stdout)
            .expect("artifact parsed from stdout");

        assert_eq!(
            artifact.pointer("/raw_posts/0/title").and_then(Value::as_str),
            Some("Lead")
        );
    }

    #[test]
    fn connector_source_recovery_normalizes_raw_connector_wrapper() {
        let node = source_node();
        let stdout = r#"{"data":{"results":[{"response":{"data_preview":{"posts":[{"title":"Preview only","url":"https://example.test"}]}}}]}}"#;

        let artifact = automation_parse_connector_source_artifact_from_text(&node, stdout)
            .expect("raw connector wrapper normalized");

        assert_eq!(
            artifact.pointer("/raw_posts/0/title").and_then(Value::as_str),
            Some("Preview only")
        );
        assert_eq!(
            artifact.get("query").and_then(Value::as_str),
            Some("local llm data privacy corporate")
        );
    }

    #[test]
    fn connector_source_recovery_normalizes_structured_connector_payload() {
        let node = source_node();
        let payload = json!({
            "data": {
                "results": [{
                    "response": {
                        "data": {
                            "posts": [{
                                "title": "Structured row",
                                "url": "https://example.test/structured"
                            }]
                        }
                    }
                }]
            }
        });

        let artifact = automation_connector_source_artifact_from_value(&node, &payload)
            .expect("structured connector payload normalized");

        assert_eq!(
            artifact.pointer("/raw_posts/0/title").and_then(Value::as_str),
            Some("Structured row")
        );
        assert_eq!(
            artifact.get("query").and_then(Value::as_str),
            Some("local llm data privacy corporate")
        );
    }

    #[test]
    fn connector_source_recovery_normalizes_embedded_stdout_payload() {
        let node = source_node();
        let stdout = r#"{"data":{"stdout":"PATH /mnt/files/mex/source.json\n{\"success\":true,\"results\":[{\"response\":{\"data\":{\"posts\":[{\"title\":\"Embedded row\",\"url\":\"https://example.test/embedded\"}]}}}]}"}}"#;

        let artifact = automation_parse_connector_source_artifact_from_text(&node, stdout)
            .expect("embedded stdout payload normalized");

        assert_eq!(
            artifact.pointer("/raw_posts/0/title").and_then(Value::as_str),
            Some("Embedded row")
        );
    }

    #[test]
    fn connector_source_recovery_prefers_full_materializer_artifact_over_preview() {
        let node = source_node();
        let preview = json!({
            "data": {
                "results": [{
                    "response": {
                        "data_preview": {
                            "posts": [
                                {"title": "Preview one", "url": "https://example.test/preview-one"},
                                {"title": "Preview two", "url": "https://example.test/preview-two"}
                            ]
                        }
                    }
                }]
            }
        });
        let materializer_stdout = json!({
            "status": "completed",
            "artifact": {
                "status": "completed",
                "raw_posts": [
                    {"title": "Full one", "url": "https://example.test/full-one"},
                    {"title": "Full two", "url": "https://example.test/full-two"},
                    {"title": "Full three", "url": "https://example.test/full-three"}
                ]
            }
        });
        let wrapper = json!({
            "data": {
                "preview": preview,
                "stdout": materializer_stdout.to_string()
            }
        })
        .to_string();

        let artifact = automation_parse_connector_source_artifact_from_text(&node, &wrapper)
            .expect("full materializer artifact parsed");

        assert_eq!(
            artifact.get("raw_posts").and_then(Value::as_array).map(Vec::len),
            Some(3)
        );
        assert_eq!(
            artifact.pointer("/raw_posts/0/title").and_then(Value::as_str),
            Some("Full one")
        );
    }

    #[test]
    fn connector_materializer_descriptor_parses_nested_mcp_stdout() {
        let descriptor = json!({
            "status": "completed",
            "remote_artifact_path": "/mnt/files/.composio/output/tandem_connector_artifact.json",
            "row_count": 10
        });
        let result_value = json!({
            "metadata": {
                "result": {
                    "content": [{
                        "type": "text",
                        "text": json!({
                            "data": {
                                "stdout": descriptor.to_string()
                            }
                        }).to_string()
                    }]
                }
            }
        });

        let parsed = automation_connector_materializer_descriptor(&result_value)
            .expect("descriptor parsed from nested stdout");

        assert_eq!(
            parsed
                .get("remote_artifact_path")
                .and_then(Value::as_str),
            Some("/mnt/files/.composio/output/tandem_connector_artifact.json")
        );
    }

    #[test]
    fn connector_remote_materialization_skips_when_verified_output_is_valid() {
        let node = source_node();
        let capture = json!({
            "remote_hydration_required": true,
            "remote_file_paths": ["/mnt/files/mex/source.json"]
        });
        let verified_output = (
            ".tandem/runs/run/artifacts/source.json".to_string(),
            json!({
                "status": "completed",
                "raw_posts": [{
                    "title": "Already materialized",
                    "url": "https://example.test/already"
                }]
            })
            .to_string(),
            AutomationVerifiedOutputResolution {
                path: std::path::PathBuf::from(
                    "/tmp/workspace/.tandem/runs/run/artifacts/source.json",
                ),
                legacy_workspace_artifact_promoted_from: None,
                materialized_by_current_attempt: true,
                resolution_kind: AutomationVerifiedOutputResolutionKind::SessionTextRecovery,
            },
        );

        assert!(!automation_should_materialize_connector_remote_result(
            &node,
            &capture,
            Some(&verified_output)
        ));
    }

    #[test]
    fn connector_remote_materialization_runs_when_remote_hydration_required_and_output_missing() {
        let node = source_node();
        let capture = json!({
            "remote_hydration_required": true,
            "remote_file_paths": ["/mnt/files/mex/source.json"]
        });

        assert!(automation_should_materialize_connector_remote_result(
            &node,
            &capture,
            None
        ));
    }
}
