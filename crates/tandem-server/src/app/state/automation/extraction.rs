// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::Value;
use tandem_types::{MessagePart, MessageRole, Session};

pub(crate) fn automation_tool_result_output_value<'a>(
    result: Option<&'a Value>,
) -> Option<&'a Value> {
    let value = result?;
    let Some(object) = value.as_object() else {
        return Some(value);
    };
    if object.contains_key("output") || object.contains_key("metadata") {
        object.get("output")
    } else {
        Some(value)
    }
}

pub(crate) fn automation_tool_result_metadata<'a>(result: Option<&'a Value>) -> Option<&'a Value> {
    let value = result?;
    let object = value.as_object()?;
    if object.contains_key("output") || object.contains_key("metadata") {
        object.get("metadata")
    } else {
        None
    }
}

pub(crate) fn automation_tool_result_output_text(result: Option<&Value>) -> Option<String> {
    let output = automation_tool_result_output_value(result)?;
    match output {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(values) => {
            let lines = values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if lines.is_empty() {
                serde_json::to_string(output).ok()
            } else {
                Some(lines.join("\n"))
            }
        }
        other => serde_json::to_string(other).ok(),
    }
}

pub(crate) fn automation_tool_result_output_payload(result: Option<&Value>) -> Option<Value> {
    let output = automation_tool_result_output_value(result)?;
    match output {
        Value::Null => None,
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                serde_json::from_str::<Value>(trimmed)
                    .ok()
                    .or_else(|| Some(Value::String(text.clone())))
            }
        }
        other => Some(other.clone()),
    }
}

pub(crate) fn automation_tool_result_failure_reason(result: Option<&Value>) -> Option<String> {
    fn failure_from_value(value: &Value) -> Option<String> {
        match value {
            Value::Object(map) => {
                let status_failed = map
                    .get("status")
                    .and_then(Value::as_i64)
                    .is_some_and(|status| status >= 400)
                    || map
                        .get("status_code")
                        .and_then(Value::as_i64)
                        .is_some_and(|status| status >= 400);
                let named_error = map
                    .get("name")
                    .or_else(|| map.get("type"))
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.to_ascii_lowercase().contains("error"));
                let coded_error = map
                    .get("code")
                    .and_then(Value::as_str)
                    .is_some_and(|code| code.to_ascii_lowercase().contains("error"));
                let explicit_success_false = map.get("success").and_then(Value::as_bool)
                    == Some(false)
                    || map.get("successful").and_then(Value::as_bool) == Some(false)
                    || map.get("successfull").and_then(Value::as_bool) == Some(false);

                if status_failed || named_error || coded_error || explicit_success_false {
                    let message = map
                        .get("message")
                        .or_else(|| map.get("error"))
                        .or_else(|| map.get("body"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("tool result reported an API or connector error");
                    return Some(message.to_string());
                }

                if let Some(body) = map.get("body").and_then(Value::as_str) {
                    if let Ok(parsed) = serde_json::from_str::<Value>(body) {
                        if let Some(reason) = failure_from_value(&parsed) {
                            return Some(reason);
                        }
                    }
                }

                for nested in map.values() {
                    if let Some(reason) = failure_from_value(nested) {
                        return Some(reason);
                    }
                }
                None
            }
            Value::Array(rows) => rows.iter().find_map(failure_from_value),
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    return failure_from_value(&parsed);
                }
                let lowered = trimmed.to_ascii_lowercase();
                if lowered.contains("apiresponseerror")
                    || lowered.starts_with("mcp error ")
                    || lowered.contains("mcp error -")
                    || (lowered.contains("validation_error") && lowered.contains("\"status\":400"))
                    || lowered.contains("request failed (500)")
                    || lowered.contains("request failed (400)")
                {
                    return Some(trimmed.chars().take(500).collect());
                }
                None
            }
            _ => None,
        }
    }

    let payload = automation_tool_result_output_payload(result)?;
    failure_from_value(&payload)
}

pub(crate) fn extract_session_text_output(session: &Session) -> String {
    session
        .messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, MessageRole::Assistant))
        .map(|message| {
            message
                .parts
                .iter()
                .filter_map(|part| match part {
                    MessagePart::Text { text } | MessagePart::Reasoning { text } => {
                        Some(text.as_str())
                    }
                    MessagePart::ToolInvocation { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

pub(crate) fn parse_status_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut seen = std::collections::BTreeSet::<String>::new();
    let mut candidates = Vec::<String>::new();

    for candidate in std::iter::once(trimmed.to_string())
        .chain(extract_markdown_json_blocks(trimmed))
        .chain(extract_loose_json_blocks(trimmed))
    {
        let normalized = candidate.trim().to_string();
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        candidates.push(normalized);
    }

    candidates.into_iter().find_map(|candidate| {
        let value = serde_json::from_str::<Value>(&candidate).ok()?;
        if automation_json_looks_like_status_payload(&value) {
            Some(value)
        } else {
            None
        }
    })
}

pub(crate) fn extract_markdown_json_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut remainder = text;
    while let Some(start) = remainder.find("```") {
        remainder = &remainder[start + 3..];
        let Some(line_end) = remainder.find('\n') else {
            break;
        };
        let lang = remainder[..line_end].trim().to_ascii_lowercase();
        remainder = &remainder[line_end + 1..];
        let Some(end) = remainder.find("```") else {
            break;
        };
        let block = remainder[..end].trim();
        if !block.is_empty() && (lang.is_empty() || lang == "json" || lang == "javascript") {
            blocks.push(block.to_string());
        }
        remainder = &remainder[end + 3..];
    }
    blocks
}

pub(crate) fn extract_loose_json_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut start = None::<usize>;
    let mut stack = Vec::<char>::new();
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if stack.is_empty() {
                    start = Some(idx);
                }
                stack.push('}');
            }
            '[' => {
                if stack.is_empty() {
                    start = Some(idx);
                }
                stack.push(']');
            }
            '}' | ']' => {
                let Some(expected) = stack.pop() else {
                    continue;
                };
                if ch != expected {
                    stack.clear();
                    start = None;
                    continue;
                }
                if stack.is_empty() {
                    if let Some(begin) = start.take() {
                        if let Some(block) = text.get(begin..=idx) {
                            blocks.push(block.trim().to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    blocks
}

pub(crate) fn automation_session_text_is_tool_summary_fallback(raw: &str) -> bool {
    let lowered = raw.trim().to_ascii_lowercase();
    lowered.contains("model returned no final narrative text")
        || lowered.contains("tool result summary:")
}

fn automation_json_looks_like_status_payload(value: &Value) -> bool {
    let Value::Object(map) = value else {
        return false;
    };
    if !map.contains_key("status") {
        return false;
    }
    map.keys().all(|key| {
        matches!(
            key.as_str(),
            "status"
                | "approved"
                | "artifact_path"
                | "artifactPath"
                | "output_path"
                | "outputPath"
                | "participant_count"
                | "participantCount"
                | "report_path"
                | "reportPath"
                | "reason"
                | "summary"
                | "failureCode"
                | "failure_code"
                | "blockedReasonCode"
                | "blocked_reason_code"
                | "repairAttempt"
                | "repairAttemptsRemaining"
                | "repairExhausted"
                | "unmetRequirements"
                | "phase"
        )
    })
}

pub(crate) fn extract_structured_handoff_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || automation_session_text_is_tool_summary_fallback(trimmed) {
        return None;
    }

    let mut seen = std::collections::BTreeSet::<String>::new();
    let mut candidates = Vec::<String>::new();

    for candidate in std::iter::once(trimmed.to_string())
        .chain(extract_markdown_json_blocks(trimmed))
        .chain(extract_loose_json_blocks(trimmed))
    {
        let normalized = candidate.trim().to_string();
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        candidates.push(normalized);
    }

    candidates.into_iter().find_map(|candidate| {
        let value = serde_json::from_str::<Value>(&candidate).ok()?;
        if automation_json_looks_like_status_payload(&value) {
            None
        } else {
            Some(value)
        }
    })
}

pub(crate) fn extract_recoverable_json_artifact(raw: &str) -> Option<Value> {
    let handoff = extract_structured_handoff_json(raw)?;
    let nested = [
        handoff.pointer("/structured_handoff").cloned(),
        handoff.pointer("/content/structured_handoff").cloned(),
        handoff.pointer("/artifact").cloned(),
        handoff.pointer("/content/data").cloned(),
        handoff.pointer("/data").cloned(),
        handoff.pointer("/artifact/content").cloned(),
        handoff.pointer("/artifact/data").cloned(),
    ]
    .into_iter()
    .flatten()
    .find(|value| match value {
        Value::Object(_) | Value::Array(_) => !automation_json_looks_like_status_payload(value),
        _ => false,
    });
    nested.or(Some(handoff))
}

pub(crate) fn extract_recoverable_json_from_session(session: &Session) -> Option<Value> {
    let from_assistant = extract_recoverable_json_artifact(&extract_session_text_output(session));
    if from_assistant.is_some() {
        return from_assistant;
    }
    for message in session.messages.iter().rev() {
        for part in &message.parts {
            let MessagePart::ToolInvocation { result, .. } = part else {
                continue;
            };
            let Some(payload) = automation_tool_result_output_payload(result.as_ref()) else {
                continue;
            };
            match &payload {
                Value::Object(_) | Value::Array(_) => {
                    if !automation_json_looks_like_status_payload(&payload) {
                        return Some(payload);
                    }
                }
                Value::String(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Some(value) = extract_recoverable_json_artifact(trimmed) {
                        return Some(value);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

pub(crate) fn extract_recoverable_json_artifact_prefer_standup(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut seen = std::collections::BTreeSet::<String>::new();
    let mut candidates = Vec::<String>::new();

    for candidate in std::iter::once(trimmed.to_string())
        .chain(extract_markdown_json_blocks(trimmed))
        .chain(extract_loose_json_blocks(trimmed))
    {
        let normalized = candidate.trim().to_string();
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        candidates.push(normalized);
    }

    let standup_candidate = candidates.clone().into_iter().find_map(|candidate| {
        let value = serde_json::from_str::<Value>(&candidate).ok()?;
        if automation_json_looks_like_status_payload(&value) {
            return None;
        }
        let has_yesterday = value.get("yesterday").is_some();
        let has_today = value.get("today").is_some();
        if has_yesterday && has_today {
            Some(value)
        } else {
            None
        }
    });

    standup_candidate.or_else(|| {
        candidates.into_iter().find_map(|candidate| {
            let value = serde_json::from_str::<Value>(&candidate).ok()?;
            if automation_json_looks_like_status_payload(&value) {
                None
            } else {
                Some(value)
            }
        })
    })
}

pub(crate) fn detect_glob_loop(tool_telemetry: &Value) -> Option<String> {
    let tool_call_counts = tool_telemetry.get("tool_call_counts")?;
    let counts = tool_call_counts.as_object()?;
    let read_count = counts.get("read").and_then(Value::as_u64).unwrap_or(0);
    let write_count = counts.get("write").and_then(Value::as_u64).unwrap_or(0);
    let mut total_calls: u64 = 0;
    for (tool_name, count) in counts {
        if let Some(c) = count.as_u64() {
            total_calls += c;
            let normalized = tool_name.to_ascii_lowercase();
            if normalized == "glob" && c >= 10 && read_count == 0 {
                return Some(format!(
                    "Agent called `glob` {} times without reading any files. \
                     Switch to `read` to examine file contents instead of continuing to glob.",
                    c
                ));
            }
            if (normalized.contains("discover") || normalized.contains("find"))
                && c >= 15
                && read_count == 0
            {
                return Some(format!(
                    "Agent called `{}` {} times without reading files. \
                     Use `read` to examine discovered files instead of continuing discovery.",
                    tool_name, c
                ));
            }
        }
    }
    if total_calls >= 30 && write_count == 0 && read_count == 0 {
        return Some(format!(
            "Agent made {} tool calls without writing or reading any files. \
             Use `read` on relevant files and produce the required output.",
            total_calls
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_serialized_connector_api_error_result() {
        let result = json!(
            "{\"name\":\"APIResponseError\",\"code\":\"validation_error\",\"status\":400,\"body\":\"{\\\"message\\\":\\\"Invalid select value\\\"}\"}"
        );

        let reason = automation_tool_result_failure_reason(Some(&result))
            .expect("connector API errors should not count as successful tool results");

        assert!(reason.contains("Invalid select value"));
    }

    #[test]
    fn detects_mcp_error_string_result() {
        let result = json!(
            "MCP error -32602: Input validation error: Invalid arguments for tool notion-search"
        );

        let reason = automation_tool_result_failure_reason(Some(&result))
            .expect("MCP error strings should not count as successful tool results");

        assert!(reason.contains("MCP error -32602"));
    }

    #[test]
    fn leaves_successful_tool_result_unflagged() {
        let result = json!({
            "object": "page",
            "url": "https://www.notion.so/example",
        });

        assert!(automation_tool_result_failure_reason(Some(&result)).is_none());
    }
}
