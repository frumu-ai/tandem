use super::*;

pub(crate) fn normalize_tool_args(
    tool_name: &str,
    raw_args: Value,
    latest_user_text: &str,
    latest_assistant_context: &str,
) -> NormalizedToolArgs {
    normalize_tool_args_with_mode(
        tool_name,
        raw_args,
        latest_user_text,
        latest_assistant_context,
        WritePathRecoveryMode::Heuristic,
    )
}

pub(crate) fn normalize_tool_args_with_mode(
    tool_name: &str,
    raw_args: Value,
    latest_user_text: &str,
    latest_assistant_context: &str,
    write_path_recovery_mode: WritePathRecoveryMode,
) -> NormalizedToolArgs {
    let normalized_tool = normalize_tool_name(tool_name);
    let original_args = raw_args.clone();
    let mut args = raw_args;
    let mut args_source = if args.is_string() {
        "provider_string".to_string()
    } else {
        "provider_json".to_string()
    };
    let mut args_integrity = "ok".to_string();
    let raw_args_state = classify_raw_tool_args_state(&args);
    let mut query = None;
    let mut missing_terminal = false;
    let mut missing_terminal_reason = None;

    if normalized_tool == "websearch" {
        if let Some(found) = extract_websearch_query(&args) {
            query = Some(found);
            args = set_websearch_query_and_source(args, query.clone(), "tool_args");
        } else if let Some(inferred) = infer_websearch_query_from_text(latest_user_text) {
            args_source = "inferred_from_user".to_string();
            args_integrity = "recovered".to_string();
            query = Some(inferred);
            args = set_websearch_query_and_source(args, query.clone(), "inferred_from_user");
        } else if let Some(recovered) = infer_websearch_query_from_text(latest_assistant_context) {
            args_source = "recovered_from_context".to_string();
            args_integrity = "recovered".to_string();
            query = Some(recovered);
            args = set_websearch_query_and_source(args, query.clone(), "recovered_from_context");
        } else {
            args_source = "missing".to_string();
            args_integrity = "empty".to_string();
            missing_terminal = true;
            missing_terminal_reason = Some("WEBSEARCH_QUERY_MISSING".to_string());
        }
    } else if tool_name_requires_query_arg(&normalized_tool) {
        if let Some(found) = extract_query_arg(&args) {
            query = Some(found);
            args = set_query_arg(args, query.clone(), "tool_args");
        } else if let Some(inferred) = infer_query_from_text(latest_user_text) {
            args_source = "inferred_from_user".to_string();
            args_integrity = "recovered".to_string();
            query = Some(inferred);
            args = set_query_arg(args, query.clone(), "inferred_from_user");
        } else if let Some(recovered) = infer_query_from_text(latest_assistant_context) {
            args_source = "recovered_from_context".to_string();
            args_integrity = "recovered".to_string();
            query = Some(recovered);
            args = set_query_arg(args, query.clone(), "recovered_from_context");
        } else {
            args_source = "missing".to_string();
            args_integrity = "empty".to_string();
            missing_terminal = true;
            missing_terminal_reason = Some("QUERY_MISSING".to_string());
        }
    } else if tool_name_requires_doc_path_arg(&normalized_tool) {
        if let Some(path) = extract_doc_path_arg(&args) {
            args = set_doc_path_arg(args, path);
        } else if let Some(inferred) = infer_doc_path_from_text(latest_user_text) {
            args_source = "inferred_from_user".to_string();
            args_integrity = "recovered".to_string();
            args = set_doc_path_arg(args, inferred);
        } else if let Some(recovered) = infer_doc_path_from_text(latest_assistant_context) {
            args_source = "recovered_from_context".to_string();
            args_integrity = "recovered".to_string();
            args = set_doc_path_arg(args, recovered);
        } else {
            args_source = "missing".to_string();
            args_integrity = "empty".to_string();
            missing_terminal = true;
            missing_terminal_reason = Some("DOC_PATH_MISSING".to_string());
        }
    } else if is_shell_tool_name(&normalized_tool) {
        if let Some(command) = extract_shell_command(&args) {
            args = set_shell_command(args, command);
        } else if let Some(inferred) = infer_shell_command_from_text(latest_assistant_context) {
            args_source = "inferred_from_context".to_string();
            args_integrity = "recovered".to_string();
            args = set_shell_command(args, inferred);
        } else if let Some(inferred) = infer_shell_command_from_text(latest_user_text) {
            args_source = "inferred_from_user".to_string();
            args_integrity = "recovered".to_string();
            args = set_shell_command(args, inferred);
        } else {
            args_source = "missing".to_string();
            args_integrity = "empty".to_string();
            missing_terminal = true;
            missing_terminal_reason = Some("BASH_COMMAND_MISSING".to_string());
        }
    } else if matches!(normalized_tool.as_str(), "read" | "write" | "edit") {
        if let Some(path) = extract_file_path_arg(&args) {
            args = set_file_path_arg(args, path);
        } else if normalized_tool == "write" || normalized_tool == "edit" {
            // Check if the model explicitly provided a non-trivial path argument that was
            // rejected by sanitization. In that case, do NOT silently recover with a
            // heuristic path — that creates garbage files. Return a terminal error so the
            // model can retry with a correct path.
            //
            // We exclude trivial/placeholder paths ("./", ".", "") because those indicate
            // the model didn't actually know the path and recovery is appropriate.
            let model_explicit_path_value = args
                .as_object()
                .and_then(|obj| obj.get("path"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|p| !p.is_empty());
            let path_is_trivial_placeholder = model_explicit_path_value
                .is_some_and(|p| matches!(p, "./" | "." | ".." | "/" | "~"));
            let model_explicitly_set_nontrivial_path = model_explicit_path_value
                .is_some_and(|p| p.len() > 2)
                && !path_is_trivial_placeholder;
            if model_explicitly_set_nontrivial_path {
                args_source = "rejected".to_string();
                args_integrity = "rejected_path".to_string();
                missing_terminal = true;
                missing_terminal_reason = Some("WRITE_PATH_REJECTED".to_string());
            } else if let Some(inferred) =
                infer_required_output_target_path_from_text(latest_user_text).or_else(|| {
                    infer_required_output_target_path_from_text(latest_assistant_context)
                })
            {
                args_source = "recovered_from_context".to_string();
                args_integrity = "recovered".to_string();
                args = set_file_path_arg(args, inferred);
            } else if write_path_recovery_mode == WritePathRecoveryMode::Heuristic {
                if let Some(inferred) = infer_write_file_path_from_text(latest_user_text) {
                    args_source = "inferred_from_user".to_string();
                    args_integrity = "recovered".to_string();
                    args = set_file_path_arg(args, inferred);
                } else {
                    args_source = "missing".to_string();
                    args_integrity = "empty".to_string();
                    missing_terminal = true;
                    missing_terminal_reason = Some("FILE_PATH_MISSING".to_string());
                }
            } else {
                args_source = "missing".to_string();
                args_integrity = "empty".to_string();
                missing_terminal = true;
                missing_terminal_reason = Some("FILE_PATH_MISSING".to_string());
            }
        } else if let Some(inferred) = infer_file_path_from_text(latest_user_text) {
            args_source = "inferred_from_user".to_string();
            args_integrity = "recovered".to_string();
            args = set_file_path_arg(args, inferred);
        } else {
            args_source = "missing".to_string();
            args_integrity = "empty".to_string();
            missing_terminal = true;
            missing_terminal_reason = Some("FILE_PATH_MISSING".to_string());
        }

        if !missing_terminal && normalized_tool == "write" {
            if let Some(content) = extract_write_content_arg(&args) {
                args = set_write_content_arg(args, content);
            } else if let Some(recovered) =
                infer_write_content_from_assistant_context(latest_assistant_context)
            {
                args_source = "recovered_from_context".to_string();
                args_integrity = "recovered".to_string();
                args = set_write_content_arg(args, recovered);
            } else {
                args_source = "missing".to_string();
                args_integrity = "empty".to_string();
                missing_terminal = true;
                missing_terminal_reason = Some("WRITE_CONTENT_MISSING".to_string());
            }
        }
    } else if matches!(normalized_tool.as_str(), "webfetch" | "webfetch_html") {
        if let Some(url) = extract_webfetch_url_arg(&args) {
            args = set_webfetch_url_arg(args, url);
        } else if let Some(inferred) = infer_url_from_text(latest_assistant_context) {
            args_source = "inferred_from_context".to_string();
            args_integrity = "recovered".to_string();
            args = set_webfetch_url_arg(args, inferred);
        } else if let Some(inferred) = infer_url_from_text(latest_user_text) {
            args_source = "inferred_from_user".to_string();
            args_integrity = "recovered".to_string();
            args = set_webfetch_url_arg(args, inferred);
        } else {
            args_source = "missing".to_string();
            args_integrity = "empty".to_string();
            missing_terminal = true;
            missing_terminal_reason = Some("WEBFETCH_URL_MISSING".to_string());
        }
    } else if tool_name_requires_task_arg(&normalized_tool) {
        if let Some(task) = extract_task_arg(&args) {
            args = set_task_arg(args, task);
        } else if let Some(inferred) = infer_task_from_text(latest_user_text) {
            args_source = "inferred_from_user".to_string();
            args_integrity = "recovered".to_string();
            args = set_task_arg(args, inferred);
        } else if let Some(recovered) = infer_task_from_text(latest_assistant_context) {
            args_source = "recovered_from_context".to_string();
            args_integrity = "recovered".to_string();
            args = set_task_arg(args, recovered);
        } else {
            args_source = "missing".to_string();
            args_integrity = "empty".to_string();
            missing_terminal = true;
            missing_terminal_reason = Some("TASK_MISSING".to_string());
        }
    } else if normalized_tool == "pack_builder" {
        let mode = extract_pack_builder_mode_arg(&args);
        let plan_id = extract_pack_builder_plan_id_arg(&args);
        if mode.as_deref() == Some("apply") && plan_id.is_none() {
            if let Some(inferred_plan) =
                infer_pack_builder_apply_plan_id(latest_user_text, latest_assistant_context)
            {
                args_source = "recovered_from_context".to_string();
                args_integrity = "recovered".to_string();
                args = set_pack_builder_apply_args(args, inferred_plan);
            } else {
                args_source = "missing".to_string();
                args_integrity = "empty".to_string();
                missing_terminal = true;
                missing_terminal_reason = Some("PACK_BUILDER_PLAN_ID_MISSING".to_string());
            }
        } else if mode.as_deref() == Some("apply") {
            args = ensure_pack_builder_default_mode(args);
        } else if let Some(inferred_plan) =
            infer_pack_builder_apply_plan_id(latest_user_text, latest_assistant_context)
        {
            args_source = "recovered_from_context".to_string();
            args_integrity = "recovered".to_string();
            args = set_pack_builder_apply_args(args, inferred_plan);
        } else if let Some(goal) = extract_pack_builder_goal_arg(&args) {
            args = set_pack_builder_goal_arg(args, goal);
        } else if let Some(inferred) = infer_pack_builder_goal_from_text(latest_user_text) {
            args_source = "inferred_from_user".to_string();
            args_integrity = "recovered".to_string();
            args = set_pack_builder_goal_arg(args, inferred);
        } else if let Some(recovered) = infer_pack_builder_goal_from_text(latest_assistant_context)
        {
            args_source = "recovered_from_context".to_string();
            args_integrity = "recovered".to_string();
            args = set_pack_builder_goal_arg(args, recovered);
        } else {
            args_source = "missing".to_string();
            args_integrity = "empty".to_string();
            missing_terminal = true;
            missing_terminal_reason = Some("PACK_BUILDER_GOAL_MISSING".to_string());
        }
        args = ensure_pack_builder_default_mode(args);
    } else if is_email_delivery_tool_name(&normalized_tool) {
        let sanitized = sanitize_email_attachment_args(args);
        if sanitized != original_args {
            args_source = "sanitized_attachment".to_string();
            args_integrity = "recovered".to_string();
        }
        args = sanitized;
    }

    NormalizedToolArgs {
        args,
        args_source,
        args_integrity,
        raw_args_state,
        query,
        missing_terminal,
        missing_terminal_reason,
    }
}

pub(crate) fn classify_raw_tool_args_state(raw_args: &Value) -> RawToolArgsState {
    match raw_args {
        Value::Null => RawToolArgsState::Empty,
        Value::Object(obj) => {
            if obj.is_empty() {
                RawToolArgsState::Empty
            } else {
                RawToolArgsState::Present
            }
        }
        Value::Array(items) => {
            if items.is_empty() {
                RawToolArgsState::Empty
            } else {
                RawToolArgsState::Present
            }
        }
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return RawToolArgsState::Empty;
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                return classify_raw_tool_args_state(&parsed);
            }
            if parse_function_style_args(trimmed).is_empty() {
                return RawToolArgsState::Unparseable;
            }
            RawToolArgsState::Present
        }
        _ => RawToolArgsState::Present,
    }
}

fn args_missing_or_empty(args: &Value) -> bool {
    match args {
        Value::Null => true,
        Value::Object(obj) => obj.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::String(raw) => raw.trim().is_empty(),
        _ => false,
    }
}

pub(crate) fn persisted_failed_tool_args(raw_args: &Value, normalized_args: &Value) -> Value {
    if args_missing_or_empty(raw_args) && !args_missing_or_empty(normalized_args) {
        normalized_args.clone()
    } else {
        raw_args.clone()
    }
}

pub(crate) fn provider_specific_write_reason(
    tool: &str,
    missing_reason: &str,
    raw_args_state: RawToolArgsState,
) -> Option<String> {
    if tool != "write"
        || !matches!(
            missing_reason,
            "FILE_PATH_MISSING" | "WRITE_CONTENT_MISSING"
        )
    {
        return None;
    }
    match raw_args_state {
        RawToolArgsState::Empty => Some("WRITE_ARGS_EMPTY_FROM_PROVIDER".to_string()),
        RawToolArgsState::Unparseable => Some("WRITE_ARGS_UNPARSEABLE_FROM_PROVIDER".to_string()),
        RawToolArgsState::Present => None,
    }
}

pub(crate) fn is_shell_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name.trim().to_ascii_lowercase().as_str(),
        "bash" | "shell" | "powershell" | "cmd"
    )
}

fn email_tool_name_tokens(tool_name: &str) -> Vec<String> {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>()
}

fn email_tool_name_compact(tool_name: &str) -> String {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
}

pub(crate) fn is_email_delivery_tool_name(tool_name: &str) -> bool {
    let tokens = email_tool_name_tokens(tool_name);
    let compact = email_tool_name_compact(tool_name);
    let looks_like_email_provider = tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "email"
                | "mail"
                | "gmail"
                | "outlook"
                | "smtp"
                | "imap"
                | "inbox"
                | "mailbox"
                | "mailer"
                | "exchange"
                | "sendgrid"
                | "mailgun"
                | "postmark"
                | "resend"
                | "ses"
        )
    });
    if !looks_like_email_provider {
        return false;
    }
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "send" | "deliver" | "reply" | "draft" | "compose" | "create"
        )
    }) || compact.contains("sendemail")
        || compact.contains("emailsend")
        || compact.contains("replyemail")
        || compact.contains("emailreply")
        || compact.contains("draftemail")
        || compact.contains("emaildraft")
        || compact.contains("composeemail")
        || compact.contains("emailcompose")
        || compact.contains("createemaildraft")
        || compact.contains("emailcreatedraft")
}

fn sanitize_email_attachment_args(args: Value) -> Value {
    let mut obj = match args {
        Value::Object(map) => map,
        other => return other,
    };
    if let Some(Value::Object(attachment)) = obj.get("attachment") {
        let s3key = attachment
            .get("s3key")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if s3key.is_empty() {
            obj.remove("attachment");
        }
    } else if obj.get("attachment").is_some() && obj.get("attachment").is_some_and(Value::is_null) {
        obj.remove("attachment");
    }
    if let Some(Value::Array(attachments)) = obj.get_mut("attachments") {
        attachments.retain(|entry| {
            entry
                .get("s3key")
                .and_then(Value::as_str)
                .map(str::trim)
                .map(|value| !value.is_empty())
                .unwrap_or(false)
        });
        if attachments.is_empty() {
            obj.remove("attachments");
        }
    }
    Value::Object(obj)
}

pub(crate) fn set_file_path_arg(args: Value, path: String) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    obj.insert("path".to_string(), Value::String(path));
    Value::Object(obj)
}

fn normalize_workspace_alias_path(path: &str, workspace_root: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.replace('\\', "/");
    if normalized == "/workspace" {
        return Some(workspace_root.to_string());
    }
    if let Some(rest) = normalized.strip_prefix("/workspace/") {
        if rest.trim().is_empty() {
            return Some(workspace_root.to_string());
        }
        return Some(rest.trim().to_string());
    }
    None
}

pub(crate) fn rewrite_workspace_alias_tool_args(
    tool: &str,
    args: Value,
    workspace_root: &str,
) -> Value {
    let normalized_tool = normalize_tool_name(tool);
    if !matches!(normalized_tool.as_str(), "read" | "write" | "edit") {
        return args;
    }
    let Some(path) = extract_file_path_arg(&args) else {
        return args;
    };
    let Some(rewritten) = normalize_workspace_alias_path(&path, workspace_root) else {
        return args;
    };
    set_file_path_arg(args, rewritten)
}

fn set_write_content_arg(args: Value, content: String) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    obj.insert("content".to_string(), Value::String(content));
    Value::Object(obj)
}

fn extract_file_path_arg(args: &Value) -> Option<String> {
    extract_file_path_arg_internal(args, 0)
}

fn extract_write_content_arg(args: &Value) -> Option<String> {
    extract_write_content_arg_internal(args, 0)
}

fn extract_file_path_arg_internal(args: &Value, depth: usize) -> Option<String> {
    if depth > 5 {
        return None;
    }

    match args {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            // If the provider sent plain string args, treat it as a path directly.
            if !(trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"')) {
                return sanitize_path_candidate(trimmed);
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                return extract_file_path_arg_internal(&parsed, depth + 1);
            }
            sanitize_path_candidate(trimmed)
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| extract_file_path_arg_internal(item, depth + 1)),
        Value::Object(obj) => {
            for key in FILE_PATH_KEYS {
                if let Some(raw) = obj.get(key).and_then(|v| v.as_str()) {
                    if let Some(path) = sanitize_path_candidate(raw) {
                        return Some(path);
                    }
                }
            }
            for container in NESTED_ARGS_KEYS {
                if let Some(nested) = obj.get(container) {
                    if let Some(path) = extract_file_path_arg_internal(nested, depth + 1) {
                        return Some(path);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_write_content_arg_internal(args: &Value, depth: usize) -> Option<String> {
    if depth > 5 {
        return None;
    }

    match args {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                return extract_write_content_arg_internal(&parsed, depth + 1);
            }
            // Some providers collapse args to a plain string. Recover as content only when
            // it does not look like a standalone file path token.
            if sanitize_path_candidate(trimmed).is_some()
                && !trimmed.contains('\n')
                && trimmed.split_whitespace().count() <= 3
            {
                return None;
            }
            Some(trimmed.to_string())
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| extract_write_content_arg_internal(item, depth + 1)),
        Value::Object(obj) => {
            for key in WRITE_CONTENT_KEYS {
                if let Some(value) = obj.get(key) {
                    if let Some(raw) = value.as_str() {
                        if !raw.is_empty() {
                            return Some(raw.to_string());
                        }
                    } else if let Some(recovered) =
                        extract_write_content_arg_internal(value, depth + 1)
                    {
                        return Some(recovered);
                    }
                }
            }
            for container in NESTED_ARGS_KEYS {
                if let Some(nested) = obj.get(container) {
                    if let Some(content) = extract_write_content_arg_internal(nested, depth + 1) {
                        return Some(content);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn infer_write_content_from_assistant_context(latest_assistant_context: &str) -> Option<String> {
    let text = latest_assistant_context.trim();
    if text.len() < 32 {
        return None;
    }
    Some(text.to_string())
}

fn set_shell_command(args: Value, command: String) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    obj.insert("command".to_string(), Value::String(command));
    Value::Object(obj)
}

pub(crate) fn extract_shell_command(args: &Value) -> Option<String> {
    extract_shell_command_internal(args, 0)
}

fn extract_shell_command_internal(args: &Value, depth: usize) -> Option<String> {
    if depth > 5 {
        return None;
    }

    match args {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if !(trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"')) {
                return sanitize_shell_command_candidate(trimmed);
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                return extract_shell_command_internal(&parsed, depth + 1);
            }
            sanitize_shell_command_candidate(trimmed)
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| extract_shell_command_internal(item, depth + 1)),
        Value::Object(obj) => {
            for key in SHELL_COMMAND_KEYS {
                if let Some(raw) = obj.get(key).and_then(|v| v.as_str()) {
                    if let Some(command) = sanitize_shell_command_candidate(raw) {
                        return Some(command);
                    }
                }
            }
            for container in NESTED_ARGS_KEYS {
                if let Some(nested) = obj.get(container) {
                    if let Some(command) = extract_shell_command_internal(nested, depth + 1) {
                        return Some(command);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn infer_shell_command_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Prefer explicit backtick commands first.
    let mut in_tick = false;
    let mut tick_buf = String::new();
    for ch in trimmed.chars() {
        if ch == '`' {
            if in_tick {
                if let Some(candidate) = sanitize_shell_command_candidate(&tick_buf) {
                    if looks_like_shell_command(&candidate) {
                        return Some(candidate);
                    }
                }
                tick_buf.clear();
            }
            in_tick = !in_tick;
            continue;
        }
        if in_tick {
            tick_buf.push(ch);
        }
    }

    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        for prefix in [
            "run ",
            "execute ",
            "call ",
            "use bash ",
            "use shell ",
            "bash ",
            "shell ",
            "powershell ",
            "pwsh ",
        ] {
            if lower.starts_with(prefix) {
                let candidate = line[prefix.len()..].trim();
                if let Some(command) = sanitize_shell_command_candidate(candidate) {
                    if looks_like_shell_command(&command) {
                        return Some(command);
                    }
                }
            }
        }
    }

    None
}

fn set_websearch_query_and_source(args: Value, query: Option<String>, query_source: &str) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    if let Some(q) = query {
        obj.insert("query".to_string(), Value::String(q));
    }
    obj.insert(
        "__query_source".to_string(),
        Value::String(query_source.to_string()),
    );
    Value::Object(obj)
}

fn set_webfetch_url_arg(args: Value, url: String) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    obj.insert("url".to_string(), Value::String(url));
    Value::Object(obj)
}

fn set_query_arg(args: Value, query: Option<String>, _source: &str) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    if let Some(query) = query {
        obj.insert("query".to_string(), Value::String(query));
    }
    Value::Object(obj)
}

fn set_doc_path_arg(args: Value, path: String) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    obj.insert("path".to_string(), Value::String(path));
    Value::Object(obj)
}

fn set_pack_builder_goal_arg(args: Value, goal: String) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    obj.insert("goal".to_string(), Value::String(goal));
    Value::Object(obj)
}

fn set_task_arg(args: Value, task: String) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    obj.insert("task".to_string(), Value::String(task));
    Value::Object(obj)
}

fn set_pack_builder_apply_args(args: Value, plan_id: String) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    obj.insert("mode".to_string(), Value::String("apply".to_string()));
    obj.insert("plan_id".to_string(), Value::String(plan_id));
    obj.insert(
        "approve_connector_registration".to_string(),
        Value::Bool(true),
    );
    obj.insert("approve_pack_install".to_string(), Value::Bool(true));
    obj.insert("approve_enable_routines".to_string(), Value::Bool(false));
    Value::Object(obj)
}

fn extract_pack_builder_mode_arg(args: &Value) -> Option<String> {
    for key in ["mode"] {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            let mode = value.trim().to_ascii_lowercase();
            if !mode.is_empty() {
                return Some(mode);
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            if let Some(value) = obj.get("mode").and_then(|v| v.as_str()) {
                let mode = value.trim().to_ascii_lowercase();
                if !mode.is_empty() {
                    return Some(mode);
                }
            }
        }
    }
    None
}

fn extract_pack_builder_plan_id_arg(args: &Value) -> Option<String> {
    for key in ["plan_id", "planId"] {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            let plan_id = value.trim();
            if !plan_id.is_empty() {
                return Some(plan_id.to_string());
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in ["plan_id", "planId"] {
                if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                    let plan_id = value.trim();
                    if !plan_id.is_empty() {
                        return Some(plan_id.to_string());
                    }
                }
            }
        }
    }
    None
}

fn extract_pack_builder_plan_id_from_text(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    let mut idx = 0usize;
    while idx + 5 <= bytes.len() {
        if &bytes[idx..idx + 5] != b"plan-" {
            idx += 1;
            continue;
        }
        let mut end = idx + 5;
        while end < bytes.len() {
            let ch = bytes[end] as char;
            if ch.is_ascii_alphanumeric() || ch == '-' {
                end += 1;
            } else {
                break;
            }
        }
        if end > idx + 5 {
            let candidate = &text[idx..end];
            if candidate.len() >= 10 {
                return Some(candidate.to_string());
            }
        }
        idx = end.saturating_add(1);
    }
    None
}

fn is_pack_builder_confirmation_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "confirm"
            | "confirmed"
            | "approve"
            | "approved"
            | "yes"
            | "y"
            | "ok"
            | "okay"
            | "go"
            | "go ahead"
            | "ship it"
            | "do it"
            | "apply"
            | "run it"
            | "✅"
            | "👍"
    )
}

fn infer_pack_builder_apply_plan_id(
    latest_user_text: &str,
    latest_assistant_context: &str,
) -> Option<String> {
    if let Some(plan_id) = extract_pack_builder_plan_id_from_text(latest_user_text) {
        return Some(plan_id);
    }
    if !is_pack_builder_confirmation_text(latest_user_text) {
        return None;
    }
    extract_pack_builder_plan_id_from_text(latest_assistant_context)
}

fn ensure_pack_builder_default_mode(args: Value) -> Value {
    let mut obj = args.as_object().cloned().unwrap_or_default();
    let has_mode = obj
        .get("mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|v| !v.is_empty());
    if !has_mode {
        obj.insert("mode".to_string(), Value::String("preview".to_string()));
    }
    Value::Object(obj)
}

fn extract_webfetch_url_arg(args: &Value) -> Option<String> {
    const URL_KEYS: [&str; 5] = ["url", "uri", "link", "href", "target_url"];
    for key in URL_KEYS {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            if let Some(url) = sanitize_url_candidate(value) {
                return Some(url);
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in URL_KEYS {
                if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                    if let Some(url) = sanitize_url_candidate(value) {
                        return Some(url);
                    }
                }
            }
        }
    }
    args.as_str().and_then(sanitize_url_candidate)
}

fn extract_pack_builder_goal_arg(args: &Value) -> Option<String> {
    const GOAL_KEYS: [&str; 1] = ["goal"];
    for key in GOAL_KEYS {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in GOAL_KEYS {
                if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    args.as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn extract_task_arg(args: &Value) -> Option<String> {
    const TASK_KEYS: [&str; 4] = ["task", "query", "question", "prompt"];
    for key in TASK_KEYS {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in TASK_KEYS {
                if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    args.as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn extract_websearch_query(args: &Value) -> Option<String> {
    const QUERY_KEYS: [&str; 5] = ["query", "q", "search_query", "searchQuery", "keywords"];
    for key in QUERY_KEYS {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            if let Some(query) = sanitize_websearch_query_candidate(value) {
                return Some(query);
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in QUERY_KEYS {
                if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                    if let Some(query) = sanitize_websearch_query_candidate(value) {
                        return Some(query);
                    }
                }
            }
        }
    }
    args.as_str().and_then(sanitize_websearch_query_candidate)
}

fn extract_query_arg(args: &Value) -> Option<String> {
    const QUERY_KEYS: [&str; 5] = ["query", "q", "search_query", "searchQuery", "keywords"];
    for key in QUERY_KEYS {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in QUERY_KEYS {
                if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    args.as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_doc_path_arg(args: &Value) -> Option<String> {
    const PATH_KEYS: [&str; 4] = ["path", "url", "doc", "page"];
    for key in PATH_KEYS {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            if let Some(path) = sanitize_doc_path_candidate(value) {
                return Some(path);
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in PATH_KEYS {
                if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                    if let Some(path) = sanitize_doc_path_candidate(value) {
                        return Some(path);
                    }
                }
            }
        }
    }
    args.as_str().and_then(sanitize_doc_path_candidate)
}

pub(crate) fn sanitize_websearch_query_candidate(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if let Some(start) = lower.find("<arg_value>") {
        let value_start = start + "<arg_value>".len();
        let tail = &trimmed[value_start..];
        let value = if let Some(end) = tail.to_ascii_lowercase().find("</arg_value>") {
            &tail[..end]
        } else {
            tail
        };
        let cleaned = value.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }

    let without_wrappers = trimmed
        .replace("<arg_key>", " ")
        .replace("</arg_key>", " ")
        .replace("<arg_value>", " ")
        .replace("</arg_value>", " ");
    let collapsed = without_wrappers
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }

    let collapsed_lower = collapsed.to_ascii_lowercase();
    if let Some(rest) = collapsed_lower.strip_prefix("websearch query ") {
        let offset = collapsed.len() - rest.len();
        let q = collapsed[offset..].trim();
        if !q.is_empty() {
            return Some(q.to_string());
        }
    }
    if let Some(rest) = collapsed_lower.strip_prefix("query ") {
        let offset = collapsed.len() - rest.len();
        let q = collapsed[offset..].trim();
        if !q.is_empty() {
            return Some(q.to_string());
        }
    }

    Some(collapsed)
}

fn infer_websearch_query_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();
    const PREFIXES: [&str; 11] = [
        "web search",
        "websearch",
        "search web for",
        "search web",
        "search for",
        "search",
        "look up",
        "lookup",
        "find",
        "web lookup",
        "query",
    ];

    let mut candidate = trimmed;
    for prefix in PREFIXES {
        if lower.starts_with(prefix) && lower.len() >= prefix.len() {
            let remainder = trimmed[prefix.len()..]
                .trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '-');
            candidate = remainder;
            break;
        }
    }

    let normalized = candidate
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace())
        .trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?'))
        .trim()
        .to_string();

    if normalized.split_whitespace().count() < 2 {
        return None;
    }
    Some(normalized)
}

fn infer_file_path_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut candidates: Vec<String> = Vec::new();

    // Prefer backtick-delimited paths when available.
    let mut in_tick = false;
    let mut tick_buf = String::new();
    for ch in trimmed.chars() {
        if ch == '`' {
            if in_tick {
                let cand = sanitize_path_candidate(&tick_buf);
                if let Some(path) = cand {
                    candidates.push(path);
                }
                tick_buf.clear();
            }
            in_tick = !in_tick;
            continue;
        }
        if in_tick {
            tick_buf.push(ch);
        }
    }

    // Fallback: scan whitespace tokens.
    for raw in trimmed.split_whitespace() {
        if let Some(path) = sanitize_path_candidate(raw) {
            candidates.push(path);
        }
    }

    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for candidate in candidates {
        if seen.insert(candidate.clone()) {
            deduped.push(candidate);
        }
    }

    deduped.into_iter().next()
}

fn infer_workspace_root_from_text(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        let value = trimmed.strip_prefix("Workspace:")?.trim();
        sanitize_path_candidate(value)
    })
}

pub(crate) fn infer_required_output_target_path_from_text(text: &str) -> Option<String> {
    // Format 1: structured JSON marker used by regular sessions
    //   "Required output target: {"path": "some-file.md"}"
    let marker = "Required output target:";
    if let Some(idx) = text.find(marker) {
        let tail = text[idx + marker.len()..].trim_start();
        if let Some(start) = tail.find('{') {
            let json_candidate = tail[start..]
                .lines()
                .take_while(|line| {
                    let trimmed = line.trim();
                    !(trimmed.is_empty() && !trimmed.starts_with('{'))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if let Ok(parsed) = serde_json::from_str::<Value>(&json_candidate) {
                if let Some(path) = parsed.get("path").and_then(|v| v.as_str()) {
                    if let Some(clean) = sanitize_explicit_output_target_path(path) {
                        return Some(clean);
                    }
                }
            }
        }
    }
    // Format 2: automation prompt "Required Workspace Output" section
    //   "Create or update `some-file.md` relative to the workspace root."
    let auto_marker = "Create or update `";
    if let Some(idx) = text.find(auto_marker) {
        let after = &text[idx + auto_marker.len()..];
        if let Some(end) = after.find('`') {
            let path = after[..end].trim();
            if let Some(clean) = sanitize_explicit_output_target_path(path) {
                return Some(clean);
            }
        }
    }
    None
}

fn infer_write_file_path_from_text(text: &str) -> Option<String> {
    let inferred = infer_file_path_from_text(text)?;
    let workspace_root = infer_workspace_root_from_text(text);
    if workspace_root
        .as_deref()
        .is_some_and(|root| root == inferred)
    {
        return None;
    }
    Some(inferred)
}

fn infer_url_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut candidates: Vec<String> = Vec::new();

    // Prefer backtick-delimited URLs when available.
    let mut in_tick = false;
    let mut tick_buf = String::new();
    for ch in trimmed.chars() {
        if ch == '`' {
            if in_tick {
                if let Some(url) = sanitize_url_candidate(&tick_buf) {
                    candidates.push(url);
                }
                tick_buf.clear();
            }
            in_tick = !in_tick;
            continue;
        }
        if in_tick {
            tick_buf.push(ch);
        }
    }

    // Fallback: scan whitespace tokens.
    for raw in trimmed.split_whitespace() {
        if let Some(url) = sanitize_url_candidate(raw) {
            candidates.push(url);
        }
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .find(|candidate| seen.insert(candidate.clone()))
}

fn infer_pack_builder_goal_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn infer_task_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn infer_query_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn infer_doc_path_from_text(text: &str) -> Option<String> {
    if let Some(url) = infer_url_from_text(text) {
        return Some(url);
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut candidates: Vec<String> = Vec::new();

    let mut in_tick = false;
    let mut tick_buf = String::new();
    for ch in trimmed.chars() {
        if ch == '`' {
            if in_tick {
                if let Some(path) = sanitize_doc_path_candidate(&tick_buf) {
                    candidates.push(path);
                }
                tick_buf.clear();
            }
            in_tick = !in_tick;
            continue;
        }
        if in_tick {
            tick_buf.push(ch);
        }
    }

    for raw in trimmed.split_whitespace() {
        if let Some(path) = sanitize_doc_path_candidate(raw) {
            candidates.push(path);
        }
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .find(|candidate| seen.insert(candidate.clone()))
}

fn tool_name_requires_task_arg(tool_name: &str) -> bool {
    let normalized = normalize_tool_name(tool_name);
    normalized == "answer_how_to" || normalized.ends_with(".answer_how_to")
}

fn tool_name_requires_query_arg(tool_name: &str) -> bool {
    let normalized = normalize_tool_name(tool_name);
    normalized == "search_docs" || normalized.ends_with(".search_docs")
}

fn tool_name_requires_doc_path_arg(tool_name: &str) -> bool {
    let normalized = normalize_tool_name(tool_name);
    normalized == "get_doc" || normalized.ends_with(".get_doc")
}

fn sanitize_url_candidate(raw: &str) -> Option<String> {
    let token = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '*' | '|'))
        .trim_start_matches(['(', '[', '{', '<'])
        .trim_end_matches([',', ';', ':', ')', ']', '}', '>'])
        .trim_end_matches('.')
        .trim();

    if token.is_empty() {
        return None;
    }
    let lower = token.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    Some(token.to_string())
}

fn sanitize_doc_path_candidate(raw: &str) -> Option<String> {
    let token = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '*' | '|'))
        .trim_start_matches(['(', '[', '{', '<'])
        .trim_end_matches([',', ';', ':', ')', ']', '}', '>'])
        .trim_end_matches('.')
        .trim();

    if token.is_empty() {
        return None;
    }

    if let Some(url) = sanitize_url_candidate(token) {
        return Some(url);
    }

    let lower = token.to_ascii_lowercase();
    if token.starts_with('/')
        || token.starts_with("./")
        || token.starts_with("../")
        || lower.starts_with("start-here")
        || lower.starts_with("sdk/")
        || lower.starts_with("desktop/")
        || lower.starts_with("control-panel/")
        || lower.starts_with("reference/")
    {
        return Some(token.to_string());
    }

    None
}

fn clean_path_candidate_token(raw: &str) -> Option<String> {
    let token = raw.trim();
    let token = token.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '*' | '|'));
    let token = token.trim_start_matches(['(', '[', '{', '<']);
    let token = token.trim_end_matches([',', ';', ':', ')', ']', '}', '>']);
    let token = token.trim_end_matches('.').trim();

    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

fn sanitize_explicit_output_target_path(raw: &str) -> Option<String> {
    let token = clean_path_candidate_token(raw)?;
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return None;
    }
    if is_malformed_tool_path_token(&token) {
        return None;
    }
    if is_root_only_path_token(&token) {
        return None;
    }
    if is_placeholder_path_token(&token) {
        return None;
    }
    if token.ends_with('/') || token.ends_with('\\') {
        return None;
    }
    Some(token.to_string())
}

fn sanitize_path_candidate(raw: &str) -> Option<String> {
    let token = clean_path_candidate_token(raw)?;
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return None;
    }
    if is_malformed_tool_path_token(token.as_str()) {
        return None;
    }
    if is_root_only_path_token(token.as_str()) {
        return None;
    }
    if is_placeholder_path_token(token.as_str()) {
        return None;
    }
    if token.ends_with('/') || token.ends_with('\\') {
        return None;
    }

    let looks_like_path = token.contains('/') || token.contains('\\');
    let has_file_ext = [
        ".md", ".txt", ".json", ".yaml", ".yml", ".toml", ".rs", ".ts", ".tsx", ".js", ".jsx",
        ".py", ".go", ".java", ".cpp", ".c", ".h", ".pdf", ".docx", ".pptx", ".xlsx", ".rtf",
        ".html", ".htm", ".css", ".scss", ".sass", ".less", ".svg", ".xml", ".sql", ".sh",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext));

    if !looks_like_path && !has_file_ext {
        return None;
    }

    Some(token)
}

fn is_placeholder_path_token(token: &str) -> bool {
    let lowered = token.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return true;
    }
    matches!(
        lowered.as_str(),
        "files/directories"
            | "file/directory"
            | "relative/or/absolute/path"
            | "path/to/file"
            | "path/to/your/file"
            | "tool/policy"
            | "tools/policy"
            | "the expected artifact file"
            | "workspace/file"
    )
}

fn is_malformed_tool_path_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    // XML-ish tool-call wrappers emitted by some model responses.
    if lower.contains("<tool_call")
        || lower.contains("</tool_call")
        || lower.contains("<function=")
        || lower.contains("<parameter=")
        || lower.contains("</function>")
        || lower.contains("</parameter>")
    {
        return true;
    }
    // Multiline payloads are not valid single file paths.
    if token.contains('\n') || token.contains('\r') {
        return true;
    }
    // Glob patterns are not concrete file paths for read/write/edit.
    if token.contains('*') || token.contains('?') {
        return true;
    }
    // Context object IDs from runtime_context_partition bindings are not file paths.
    // These look like "ctx:wfplan-...:assess:assess.artifact" and the model sometimes
    // confuses them for filesystem paths.
    if lower.starts_with("ctx:") {
        return true;
    }
    // Colon-separated identifiers that look like context bindings (e.g. "routine:assess:artifact")
    // but aren't Windows drive paths (which are caught above).
    if token.matches(':').count() >= 2 {
        return true;
    }
    false
}

fn is_root_only_path_token(token: &str) -> bool {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return true;
    }
    if matches!(trimmed, "/" | "\\" | "." | ".." | "~") {
        return true;
    }
    // Windows drive root placeholders, e.g. `C:` or `C:\`.
    let bytes = trimmed.as_bytes();
    if bytes.len() == 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        return true;
    }
    if bytes.len() == 3
        && bytes[1] == b':'
        && (bytes[0] as char).is_ascii_alphabetic()
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    false
}

fn sanitize_shell_command_candidate(raw: &str) -> Option<String> {
    let token = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | ',' | ';'))
        .trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

fn looks_like_shell_command(candidate: &str) -> bool {
    let lower = candidate.to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    let first = lower.split_whitespace().next().unwrap_or_default();
    let common = [
        "rg",
        "git",
        "cargo",
        "pnpm",
        "npm",
        "node",
        "python",
        "pytest",
        "pwsh",
        "powershell",
        "cmd",
        "dir",
        "ls",
        "cat",
        "type",
        "echo",
        "cd",
        "mkdir",
        "cp",
        "copy",
        "move",
        "del",
        "rm",
    ];
    common.contains(&first)
        || first.starts_with("get-")
        || first.starts_with("./")
        || first.starts_with(".\\")
        || lower.contains(" | ")
        || lower.contains(" && ")
        || lower.contains(" ; ")
}

const FILE_PATH_KEYS: [&str; 10] = [
    "path",
    "file_path",
    "filePath",
    "filepath",
    "filename",
    "file",
    "target",
    "targetFile",
    "absolutePath",
    "uri",
];

const SHELL_COMMAND_KEYS: [&str; 4] = ["command", "cmd", "script", "line"];

const WRITE_CONTENT_KEYS: [&str; 8] = [
    "content",
    "text",
    "body",
    "value",
    "markdown",
    "document",
    "output",
    "file_content",
];

const NESTED_ARGS_KEYS: [&str; 10] = [
    "arguments",
    "args",
    "input",
    "params",
    "payload",
    "data",
    "tool_input",
    "toolInput",
    "tool_args",
    "toolArgs",
];

pub(crate) fn tool_signature(tool_name: &str, args: &Value) -> String {
    let normalized = normalize_tool_name(tool_name);
    if normalized == "websearch" {
        let query = extract_websearch_query(args)
            .unwrap_or_default()
            .to_lowercase();
        let limit = args
            .get("limit")
            .or_else(|| args.get("numResults"))
            .or_else(|| args.get("num_results"))
            .and_then(|v| v.as_u64())
            .unwrap_or(8);
        let domains = args
            .get("domains")
            .or_else(|| args.get("domain"))
            .map(|v| v.to_string())
            .unwrap_or_default();
        let recency = args.get("recency").and_then(|v| v.as_u64()).unwrap_or(0);
        return format!("websearch:q={query}|limit={limit}|domains={domains}|recency={recency}");
    }
    format!("{}:{}", normalized, args)
}
