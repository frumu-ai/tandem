use std::collections::HashSet;

use serde_json::{json, Map, Number, Value};

use super::types::{RawToolArgsState, WritePathRecoveryMode};
use super::{is_read_only_tool, normalize_tool_name};

pub(super) fn parse_tool_invocation(input: &str) -> Option<(String, Value)> {
    let raw = input.trim();
    if !raw.starts_with("/tool ") {
        return None;
    }
    let rest = raw.trim_start_matches("/tool ").trim();
    let mut split = rest.splitn(2, ' ');
    let tool = normalize_tool_name(split.next()?.trim());
    let args = split
        .next()
        .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
        .unwrap_or_else(|| json!({}));
    Some((tool, args))
}

pub(super) fn parse_tool_invocations_from_response(input: &str) -> Vec<(String, Value)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(found) = extract_tool_call_from_value(&parsed) {
            return vec![found];
        }
    }

    if let Some(block) = extract_first_json_object(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&block) {
            if let Some(found) = extract_tool_call_from_value(&parsed) {
                return vec![found];
            }
        }
    }

    parse_function_style_tool_calls(trimmed)
}

pub(super) fn parse_tool_invocation_from_response(input: &str) -> Option<(String, Value)> {
    parse_tool_invocations_from_response(input)
        .into_iter()
        .next()
}

pub(super) fn parse_function_style_tool_calls(input: &str) -> Vec<(String, Value)> {
    let mut calls = Vec::new();
    let lower = input.to_lowercase();
    let names = [
        "todo_write",
        "todowrite",
        "update_todo_list",
        "update_todos",
    ];
    let mut cursor = 0usize;

    while cursor < lower.len() {
        let mut best: Option<(usize, &str)> = None;
        for name in names {
            let needle = format!("{name}(");
            if let Some(rel_idx) = lower[cursor..].find(&needle) {
                let idx = cursor + rel_idx;
                if best.as_ref().is_none_or(|(best_idx, _)| idx < *best_idx) {
                    best = Some((idx, name));
                }
            }
        }

        let Some((tool_start, tool_name)) = best else {
            break;
        };

        let open_paren = tool_start + tool_name.len();
        if let Some(close_paren) = find_matching_paren(input, open_paren) {
            if let Some(args_text) = input.get(open_paren + 1..close_paren) {
                let args = parse_function_style_args(args_text.trim());
                calls.push((normalize_tool_name(tool_name), Value::Object(args)));
            }
            cursor = close_paren.saturating_add(1);
        } else {
            cursor = tool_start.saturating_add(tool_name.len());
        }
    }

    calls
}

pub(super) fn find_matching_paren(input: &str, open_paren: usize) -> Option<usize> {
    if input.as_bytes().get(open_paren).copied()? != b'(' {
        return None;
    }

    let mut depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for (offset, ch) in input.get(open_paren..)?.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && (in_single || in_double) {
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if in_single || in_double {
            continue;
        }

        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open_paren + offset);
                }
            }
            _ => {}
        }
    }

    None
}

pub(super) fn parse_function_style_args(input: &str) -> Map<String, Value> {
    let mut args = Map::new();
    if input.trim().is_empty() {
        return args;
    }

    let mut parts = Vec::<String>::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut depth_paren = 0usize;
    let mut depth_bracket = 0usize;
    let mut depth_brace = 0usize;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && (in_single || in_double) {
            current.push(ch);
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            current.push(ch);
            continue;
        }
        if in_single || in_double {
            current.push(ch);
            continue;
        }

        match ch {
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            '[' => depth_bracket += 1,
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            ',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_string());
                }
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    let tail = current.trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }

    for part in parts {
        let Some((raw_key, raw_value)) = part
            .split_once('=')
            .or_else(|| part.split_once(':'))
            .map(|(k, v)| (k.trim(), v.trim()))
        else {
            continue;
        };
        let key = raw_key.trim_matches(|c| c == '"' || c == '\'' || c == '`');
        if key.is_empty() {
            continue;
        }
        if !is_valid_function_style_key(key) {
            continue;
        }
        let value = parse_scalar_like_value(raw_value);
        args.insert(key.to_string(), value);
    }

    args
}

pub(super) fn is_valid_function_style_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphanumeric() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-')
}

pub(super) fn parse_scalar_like_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }

    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        if trimmed.len() < 2 {
            return Value::String(trimmed.to_string());
        }
        return Value::String(trimmed[1..trimmed.len().saturating_sub(1)].to_string());
    }

    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if trimmed.eq_ignore_ascii_case("null") {
        return Value::Null;
    }

    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return v;
    }
    if let Ok(v) = trimmed.parse::<i64>() {
        return Value::Number(Number::from(v));
    }
    if let Ok(v) = trimmed.parse::<f64>() {
        if let Some(n) = Number::from_f64(v) {
            return Value::Number(n);
        }
    }

    Value::String(trimmed.to_string())
}

pub(super) fn recover_write_args_from_malformed_json(raw: &str) -> Option<Value> {
    let content = extract_loose_json_string_field(raw, "content")?;
    let mut obj = Map::new();
    if let Some(path) = extract_loose_json_string_field(raw, "path") {
        obj.insert("path".to_string(), Value::String(path));
    }
    obj.insert("content".to_string(), Value::String(content));
    Some(Value::Object(obj))
}

pub(super) fn extract_loose_json_string_field(input: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let start = input.find(&pattern)?;
    let remainder = input.get(start + pattern.len()..)?;
    let colon = remainder.find(':')?;
    let value = remainder.get(colon + 1..)?.trim_start();
    let value = value.strip_prefix('"')?;
    Some(parse_loose_json_string_value(value))
}

pub(super) fn parse_loose_json_string_value(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    let mut closed = false;

    while let Some(ch) = chars.next() {
        if ch == '"' {
            closed = true;
            break;
        }
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        let Some(escaped) = chars.next() else {
            out.push('\\');
            break;
        };
        match escaped {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let mut hex = String::new();
                for _ in 0..4 {
                    let Some(next) = chars.next() else {
                        break;
                    };
                    hex.push(next);
                }
                if hex.len() == 4 {
                    if let Ok(codepoint) = u16::from_str_radix(&hex, 16) {
                        if let Some(decoded) = char::from_u32(codepoint as u32) {
                            out.push(decoded);
                            continue;
                        }
                    }
                }
                out.push('\\');
                out.push('u');
                out.push_str(&hex);
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
    }

    if !closed {
        return out;
    }
    out
}

pub(super) fn normalize_todo_write_args(args: Value, completion: &str) -> Value {
    if is_todo_status_update_args(&args) {
        return args;
    }

    let mut obj = match args {
        Value::Object(map) => map,
        Value::Array(items) => {
            return json!({ "todos": normalize_todo_arg_items(items) });
        }
        Value::String(text) => {
            let derived = extract_todo_candidates_from_text(&text);
            if !derived.is_empty() {
                return json!({ "todos": derived });
            }
            return json!({});
        }
        _ => return json!({}),
    };

    if obj
        .get("todos")
        .and_then(|v| v.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false)
    {
        return Value::Object(obj);
    }

    for alias in ["tasks", "items", "list", "checklist"] {
        if let Some(items) = obj.get(alias).and_then(|v| v.as_array()) {
            let normalized = normalize_todo_arg_items(items.clone());
            if !normalized.is_empty() {
                obj.insert("todos".to_string(), Value::Array(normalized));
                return Value::Object(obj);
            }
        }
    }

    let derived = extract_todo_candidates_from_text(completion);
    if !derived.is_empty() {
        obj.insert("todos".to_string(), Value::Array(derived));
    }
    Value::Object(obj)
}

pub(super) fn normalize_todo_arg_items(items: Vec<Value>) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Value::String(text) => {
                let content = text.trim();
                if content.is_empty() {
                    None
                } else {
                    Some(json!({ "content": content }))
                }
            }
            Value::Object(mut obj) => {
                if !obj.contains_key("content") {
                    if let Some(text) = obj.get("text").cloned() {
                        obj.insert("content".to_string(), text);
                    } else if let Some(title) = obj.get("title").cloned() {
                        obj.insert("content".to_string(), title);
                    } else if let Some(name) = obj.get("name").cloned() {
                        obj.insert("content".to_string(), name);
                    }
                }
                let content = obj
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .unwrap_or("");
                if content.is_empty() {
                    None
                } else {
                    Some(Value::Object(obj))
                }
            }
            _ => None,
        })
        .collect()
}

pub(super) fn is_todo_status_update_args(args: &Value) -> bool {
    let Some(obj) = args.as_object() else {
        return false;
    };
    let has_status = obj
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_target =
        obj.get("task_id").is_some() || obj.get("todo_id").is_some() || obj.get("id").is_some();
    has_status && has_target
}

pub(super) fn is_empty_todo_write_args(args: &Value) -> bool {
    if is_todo_status_update_args(args) {
        return false;
    }
    let Some(obj) = args.as_object() else {
        return true;
    };
    !obj.get("todos")
        .and_then(|v| v.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false)
}

pub(super) fn parse_streamed_tool_args(tool_name: &str, raw_args: &str) -> Value {
    let trimmed = raw_args.trim();
    if trimmed.is_empty() {
        return json!({});
    }

    let normalized_tool = normalize_tool_name(tool_name);
    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        return normalize_streamed_tool_args(&normalized_tool, parsed, trimmed);
    }

    if normalized_tool == "write" {
        if let Some(recovered) = recover_write_args_from_malformed_json(trimmed) {
            return recovered;
        }
    }

    let kv_args = parse_function_style_args(trimmed);
    if !kv_args.is_empty() {
        return normalize_streamed_tool_args(&normalized_tool, Value::Object(kv_args), trimmed);
    }

    if normalized_tool == "websearch" {
        if let Some(query) = sanitize_websearch_query_candidate(trimmed) {
            return json!({ "query": query });
        }
        return json!({});
    }

    Value::String(trimmed.to_string())
}

pub(super) fn normalize_streamed_tool_args(tool_name: &str, parsed: Value, raw: &str) -> Value {
    let normalized_tool = normalize_tool_name(tool_name);
    if normalized_tool != "websearch" {
        return parsed;
    }

    match parsed {
        Value::Object(mut obj) => {
            if !has_websearch_query(&obj) && !raw.trim().is_empty() {
                if let Some(query) = sanitize_websearch_query_candidate(raw) {
                    obj.insert("query".to_string(), Value::String(query));
                }
            }
            Value::Object(obj)
        }
        Value::String(s) => match sanitize_websearch_query_candidate(&s) {
            Some(query) => json!({ "query": query }),
            None => json!({}),
        },
        other => other,
    }
}

fn has_websearch_query(obj: &Map<String, Value>) -> bool {
    const QUERY_KEYS: [&str; 5] = ["query", "q", "search_query", "searchQuery", "keywords"];
    QUERY_KEYS.iter().any(|key| {
        obj.get(*key)
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    })
}

pub(super) fn extract_tool_call_from_value(value: &Value) -> Option<(String, Value)> {
    if let Some(obj) = value.as_object() {
        if let Some(tool) = obj.get("tool").and_then(|v| v.as_str()) {
            return Some((
                normalize_tool_name(tool),
                obj.get("args").cloned().unwrap_or_else(|| json!({})),
            ));
        }

        if let Some(tool) = obj.get("name").and_then(|v| v.as_str()) {
            let args = obj
                .get("args")
                .cloned()
                .or_else(|| obj.get("arguments").cloned())
                .unwrap_or_else(|| json!({}));
            let normalized_tool = normalize_tool_name(tool);
            let args = if let Some(raw) = args.as_str() {
                parse_streamed_tool_args(&normalized_tool, raw)
            } else {
                args
            };
            return Some((normalized_tool, args));
        }

        for key in [
            "tool_call",
            "toolCall",
            "call",
            "function_call",
            "functionCall",
        ] {
            if let Some(nested) = obj.get(key) {
                if let Some(found) = extract_tool_call_from_value(nested) {
                    return Some(found);
                }
            }
        }

        if let Some(calls) = obj.get("tool_calls").and_then(|v| v.as_array()) {
            for call in calls {
                if let Some(found) = extract_tool_call_from_value(call) {
                    return Some(found);
                }
            }
        }
    }

    if let Some(items) = value.as_array() {
        for item in items {
            if let Some(found) = extract_tool_call_from_value(item) {
                return Some(found);
            }
        }
    }

    None
}

pub(super) fn extract_first_json_object(input: &str) -> Option<String> {
    let mut start = None;
    let mut depth = 0usize;
    for (idx, ch) in input.char_indices() {
        if ch == '{' {
            if start.is_none() {
                start = Some(idx);
            }
            depth += 1;
        } else if ch == '}' {
            if depth == 0 {
                continue;
            }
            depth -= 1;
            if depth == 0 {
                let begin = start?;
                let block = input.get(begin..=idx)?;
                return Some(block.to_string());
            }
        }
    }
    None
}

pub(super) fn extract_todo_candidates_from_text(input: &str) -> Vec<Value> {
    let mut seen = HashSet::<String>::new();
    let mut todos = Vec::new();

    for raw_line in input.lines() {
        let mut line = raw_line.trim();
        let mut structured_line = false;
        if line.is_empty() {
            continue;
        }
        if line.starts_with("```") {
            continue;
        }
        if line.ends_with(':') {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("- [ ]")
            .or_else(|| line.strip_prefix("* [ ]"))
            .or_else(|| line.strip_prefix("- [x]"))
            .or_else(|| line.strip_prefix("* [x]"))
        {
            line = rest.trim();
            structured_line = true;
        } else if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            line = rest.trim();
            structured_line = true;
        } else {
            let bytes = line.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > 0 && i + 1 < bytes.len() && (bytes[i] == b'.' || bytes[i] == b')') {
                line = line[i + 1..].trim();
                structured_line = true;
            }
        }
        if !structured_line {
            continue;
        }

        let content = line.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '*');
        if content.len() < 5 || content.len() > 180 {
            continue;
        }
        let key = content.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        todos.push(json!({ "content": content }));
        if todos.len() >= 25 {
            break;
        }
    }

    todos
}

fn is_batch_wrapper_tool_name(name: &str) -> bool {
    matches!(
        normalize_tool_name(name).as_str(),
        "default_api" | "default" | "api" | "function" | "functions" | "tool" | "tools"
    )
}

pub(super) fn non_empty_string_at<'a>(
    obj: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Option<&'a str> {
    obj.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(super) fn nested_non_empty_string_at<'a>(
    obj: &'a serde_json::Map<String, Value>,
    parent: &str,
    key: &str,
) -> Option<&'a str> {
    obj.get(parent)
        .and_then(|v| v.as_object())
        .and_then(|nested| nested.get(key))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(super) fn extract_batch_calls(args: &Value) -> Vec<(String, Value)> {
    let calls = args
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    calls
        .into_iter()
        .filter_map(|call| {
            let obj = call.as_object()?;
            let tool_raw = non_empty_string_at(obj, "tool")
                .or_else(|| nested_non_empty_string_at(obj, "tool", "name"))
                .or_else(|| nested_non_empty_string_at(obj, "function", "tool"))
                .or_else(|| nested_non_empty_string_at(obj, "function_call", "tool"))
                .or_else(|| nested_non_empty_string_at(obj, "call", "tool"));
            let name_raw = non_empty_string_at(obj, "name")
                .or_else(|| nested_non_empty_string_at(obj, "function", "name"))
                .or_else(|| nested_non_empty_string_at(obj, "function_call", "name"))
                .or_else(|| nested_non_empty_string_at(obj, "call", "name"))
                .or_else(|| nested_non_empty_string_at(obj, "tool", "name"));
            let effective = match (tool_raw, name_raw) {
                (Some(t), Some(n)) if is_batch_wrapper_tool_name(t) => n,
                (Some(t), _) => t,
                (None, Some(n)) => n,
                (None, None) => return None,
            };
            let normalized = normalize_tool_name(effective);
            let call_args = obj.get("args").cloned().unwrap_or_else(|| json!({}));
            Some((normalized, call_args))
        })
        .collect()
}

pub(super) fn is_read_only_batch_call(args: &Value) -> bool {
    let calls = extract_batch_calls(args);
    !calls.is_empty() && calls.iter().all(|(tool, _)| is_read_only_tool(tool))
}

pub(super) fn batch_tool_signature(args: &Value) -> Option<String> {
    let calls = extract_batch_calls(args);
    if calls.is_empty() {
        return None;
    }
    let parts = calls
        .into_iter()
        .map(|(tool, call_args)| tool_signature(&tool, &call_args))
        .collect::<Vec<_>>();
    Some(format!("batch:{}", parts.join("|")))
}

#[derive(Debug, Clone)]
pub(super) struct NormalizedToolArgs {
    pub(super) args: Value,
    pub(super) args_source: String,
    pub(super) args_integrity: String,
    pub(super) raw_args_state: RawToolArgsState,
    pub(super) query: Option<String>,
    pub(super) missing_terminal: bool,
    pub(super) missing_terminal_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedToolCall {
    pub(super) tool: String,
    pub(super) args: Value,
    pub(super) call_id: Option<String>,
}

mod normalize;
pub(crate) use normalize::*;
