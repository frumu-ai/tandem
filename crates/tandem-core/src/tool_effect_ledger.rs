use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::EngineEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffectLedgerPhase {
    Invocation,
    Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffectLedgerStatus {
    Started,
    Succeeded,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEffectLedgerRecord {
    pub session_id: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub tool: String,
    pub phase: ToolEffectLedgerPhase,
    pub status: ToolEffectLedgerStatus,
    pub args_summary: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn tool_effect_ledger_event(record: ToolEffectLedgerRecord) -> EngineEvent {
    EngineEvent::new(
        "tool.effect.recorded",
        json!({
            "sessionID": record.session_id.clone(),
            "messageID": record.message_id.clone(),
            "tool": record.tool.clone(),
            "record": record,
        }),
    )
}

pub fn build_tool_effect_ledger_record(
    session_id: &str,
    message_id: &str,
    tool_call_id: Option<&str>,
    tool: &str,
    phase: ToolEffectLedgerPhase,
    status: ToolEffectLedgerStatus,
    args: &Value,
    metadata: Option<&Value>,
    output: Option<&str>,
    error: Option<&str>,
) -> ToolEffectLedgerRecord {
    ToolEffectLedgerRecord {
        session_id: session_id.to_string(),
        message_id: message_id.to_string(),
        tool_call_id: tool_call_id.map(str::to_string),
        tool: tool.to_string(),
        phase,
        status,
        args_summary: summarize_args(args),
        result_summary: summarize_result(metadata, output),
        error: error
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate(value, 500)),
    }
}

fn summarize_args(args: &Value) -> Value {
    let Some(object) = args.as_object() else {
        return json!({
            "type": value_type(args),
        });
    };

    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort();

    let path = first_string_field(object, &["path", "file_path", "target_file"]);
    let url = first_string_field(object, &["url"]);
    let source_id = first_string_field(object, &["source_id", "sourceID"]);
    let document_id = first_string_field(object, &["document_id", "documentID", "doc_id"]);
    let ticket_id = first_string_field(object, &["ticket_id", "ticketID"]);
    let record_id = first_string_field(object, &["record_id", "recordID"]);
    let cwd = first_string_field(object, &["cwd", "__effective_cwd"]);
    let workspace_root = first_string_field(object, &["__workspace_root"]);
    let query_hash = first_string_field(object, &["query", "q"])
        .as_deref()
        .map(stable_hash);
    let command_hash = first_string_field(object, &["command"])
        .as_deref()
        .map(stable_hash);

    json!({
        "type": "object",
        "keys": keys,
        "field_count": object.len(),
        "path": path,
        "url": url,
        "source_id": source_id,
        "document_id": document_id,
        "ticket_id": ticket_id,
        "record_id": record_id,
        "cwd": cwd,
        "workspace_root": workspace_root,
        "query_hash": query_hash,
        "command_hash": command_hash,
    })
}

fn summarize_result(metadata: Option<&Value>, output: Option<&str>) -> Option<Value> {
    if metadata.is_none() && output.is_none() {
        return None;
    }

    let metadata_summary = metadata.map(summarize_value_shape);
    let output_chars = output.map(|value| value.chars().count());
    let output_hash = output.map(stable_hash);

    Some(json!({
        "metadata": metadata_summary,
        "output_chars": output_chars,
        "output_hash": output_hash,
    }))
}

fn summarize_value_shape(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            json!({
                "type": "object",
                "keys": keys,
                "field_count": map.len(),
            })
        }
        Value::Array(rows) => json!({
            "type": "array",
            "length": rows.len(),
        }),
        Value::String(text) => json!({
            "type": "string",
            "length": text.len(),
        }),
        Value::Number(_) => json!({"type": "number"}),
        Value::Bool(_) => json!({"type": "boolean"}),
        Value::Null => json!({"type": "null"}),
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
    }
}

fn first_string_field(
    object: &serde_json::Map<String, Value>,
    candidates: &[&str],
) -> Option<String> {
    candidates
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(|value| truncate(value, 240))
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_string()
    } else {
        value.chars().take(limit).collect::<String>()
    }
}

fn stable_hash(value: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_record_summarizes_sensitive_args_without_storing_command_text() {
        let record = build_tool_effect_ledger_record(
            "session-1",
            "message-1",
            Some("call-1"),
            "bash",
            ToolEffectLedgerPhase::Invocation,
            ToolEffectLedgerStatus::Started,
            &json!({
                "command": "cargo test -p tandem-core",
                "cwd": "/workspace",
            }),
            None,
            None,
            None,
        );

        assert_eq!(record.args_summary["command_hash"].as_str().is_some(), true);
        assert_eq!(record.args_summary["cwd"].as_str(), Some("/workspace"));
        assert!(record.args_summary.get("command").is_none());
    }

    #[test]
    fn ledger_event_contains_compact_result_summary() {
        let event = tool_effect_ledger_event(build_tool_effect_ledger_record(
            "session-1",
            "message-1",
            None,
            "write",
            ToolEffectLedgerPhase::Outcome,
            ToolEffectLedgerStatus::Succeeded,
            &json!({"path": "src/lib.rs"}),
            Some(&json!({"path": "src/lib.rs", "ok": true})),
            Some("ok"),
            None,
        ));

        assert_eq!(event.event_type, "tool.effect.recorded");
        assert_eq!(
            event.properties["record"]["result_summary"]["metadata"]["field_count"].as_u64(),
            Some(2)
        );
        assert_eq!(
            event.properties["record"]["args_summary"]["path"].as_str(),
            Some("src/lib.rs")
        );
    }

    #[test]
    fn ledger_record_preserves_safe_source_identifiers() {
        let record = build_tool_effect_ledger_record(
            "session-1",
            "message-1",
            Some("call-1"),
            "mcp.regulator.fetch_bulletin",
            ToolEffectLedgerPhase::Outcome,
            ToolEffectLedgerStatus::Succeeded,
            &json!({
                "source_id": "reg-bulletin-1",
                "document_id": "doc-123",
                "ticket_id": "case-9",
                "record_id": "record-7",
                "query": "sensitive query text"
            }),
            None,
            Some("ok"),
            None,
        );

        assert_eq!(
            record.args_summary["source_id"].as_str(),
            Some("reg-bulletin-1")
        );
        assert_eq!(record.args_summary["document_id"].as_str(), Some("doc-123"));
        assert_eq!(record.args_summary["ticket_id"].as_str(), Some("case-9"));
        assert_eq!(record.args_summary["record_id"].as_str(), Some("record-7"));
        assert!(record.args_summary.get("query").is_none());
        assert!(record.args_summary["query_hash"].as_str().is_some());
    }
}
