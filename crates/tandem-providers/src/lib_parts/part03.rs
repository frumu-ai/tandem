fn chat_message_to_openai_responses_wire(message: ChatMessage) -> serde_json::Value {
    let role = message.role.trim().to_ascii_lowercase();
    if role == "system" {
        return json!({
            "role": "developer",
            "content": message.content
        });
    }
    if role == "assistant" {
        return json!({
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": message.content,
                "annotations": []
            }]
        });
    }

    let mut content = Vec::new();
    if !message.content.trim().is_empty() {
        content.push(json!({
            "type": "input_text",
            "text": message.content
        }));
    }
    for attachment in message.attachments {
        match attachment {
            ChatAttachment::ImageUrl { url } => content.push(json!({
                "type": "input_image",
                "detail": "auto",
                "image_url": url
            })),
        }
    }
    if content.is_empty() {
        content.push(json!({
            "type": "input_text",
            "text": ""
        }));
    }

    json!({
        "role": "user",
        "content": content
    })
}

fn split_openai_responses_instructions(messages: Vec<ChatMessage>) -> (String, Vec<ChatMessage>) {
    let normalized = normalize_openai_messages(messages);
    let mut instructions = String::new();
    let mut input_messages = Vec::with_capacity(normalized.len());

    for message in normalized {
        if message.role.eq_ignore_ascii_case("system") {
            let content = message.content.trim();
            if !content.is_empty() {
                if !instructions.is_empty() {
                    instructions.push_str("\n\n");
                }
                instructions.push_str(content);
            }
            continue;
        }
        input_messages.push(message);
    }

    if instructions.trim().is_empty() {
        instructions = default_openai_responses_instructions();
    }

    (instructions, input_messages)
}

fn normalize_openai_messages(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut merged_system: Option<ChatMessage> = None;
    let mut out = Vec::with_capacity(messages.len());

    for message in messages {
        if message.role.eq_ignore_ascii_case("system") {
            let entry = merged_system.get_or_insert_with(|| ChatMessage {
                role: "system".to_string(),
                content: String::new(),
                attachments: Vec::new(),
            });
            let next_content = message.content.trim();
            if !next_content.is_empty() {
                if !entry.content.is_empty() {
                    entry.content.push_str("\n\n");
                }
                entry.content.push_str(next_content);
            }
            entry.attachments.extend(message.attachments);
            continue;
        }
        out.push(message);
    }

    if let Some(system) = merged_system {
        out.insert(0, system);
    }

    out
}

fn responses_tool_call_key(call_id: &str, item_id: &str, output_index: Option<u64>) -> String {
    let call_id = call_id.trim();
    let item_id = item_id.trim();
    if !call_id.is_empty() && !item_id.is_empty() {
        return format!("{call_id}|{item_id}");
    }
    if !call_id.is_empty() {
        return call_id.to_string();
    }
    if !item_id.is_empty() {
        return item_id.to_string();
    }
    format!("fc_{}", output_index.unwrap_or_default())
}

fn responses_tool_call_name(
    item: &serde_json::Map<String, serde_json::Value>,
    alias_to_original: &HashMap<String, String>,
) -> String {
    let raw_name = item
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| item.get("tool_name").and_then(|v| v.as_str()))
        .or_else(|| {
            item.get("function")
                .and_then(|v| v.as_object())
                .and_then(|function| function.get("name"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    if raw_name.is_empty() {
        return String::new();
    }
    canonical_openai_tool_name(&raw_name, alias_to_original)
}

fn extract_responses_reasoning_summary(
    item: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let summary = item.get("summary")?;
    let mut out = String::new();
    match summary {
        serde_json::Value::String(text) => out.push_str(text),
        serde_json::Value::Array(parts) => {
            for part in parts {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }
                    out.push_str(text);
                }
            }
        }
        _ => {}
    }
    (!out.trim().is_empty()).then_some(out)
}

fn extract_responses_usage(
    response: &serde_json::Map<String, serde_json::Value>,
) -> Option<TokenUsage> {
    let usage = response.get("usage")?.as_object()?;
    let cached_tokens = usage
        .get("input_tokens_details")
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let prompt_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .saturating_sub(cached_tokens);
    let completion_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt_tokens.saturating_add(completion_tokens));
    Some(TokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    })
}

fn map_responses_stop_reason(status: Option<&str>, incomplete_reason: Option<&str>) -> String {
    match status.map(|value| value.trim().to_ascii_lowercase()) {
        Some(ref value) if value == "completed" => "stop".to_string(),
        Some(ref value) if value == "incomplete" => "length".to_string(),
        Some(ref value) if value == "failed" || value == "cancelled" => "error".to_string(),
        Some(ref value) if value == "in_progress" || value == "queued" => "stop".to_string(),
        Some(_) | None => {
            if incomplete_reason
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            {
                "length".to_string()
            } else {
                "stop".to_string()
            }
        }
    }
}

fn model_supports_vision_input(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    [
        "vision", "gpt-4o", "gpt-4.1", "gpt-5", "omni", "gemini", "claude-3", "llava", "qwen-vl",
        "pixtral",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

fn should_default_openai_reasoning(model: &str) -> bool {
    let lower = model.trim().to_ascii_lowercase();
    lower.starts_with("gpt-5")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
}

fn normalize_base(input: &str) -> String {
    // Accept base URLs with common OpenAI-compatible suffixes and normalize to `.../v1`.
    // This prevents accidental double suffixes like `/v1/v1`.
    let mut base = input.trim().trim_end_matches('/').to_string();
    for suffix in ["/chat/completions", "/completions", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }

    // Self-heal legacy malformed values that accidentally ended up with repeated `/v1`.
    while let Some(prefix) = base.strip_suffix("/v1") {
        if prefix.ends_with("/v1") {
            base = prefix.to_string();
            continue;
        }
        break;
    }

    if base.ends_with("/v1") {
        base
    } else {
        format!("{}/v1", base.trim_end_matches('/'))
    }
}

fn normalize_plain_base(input: &str) -> String {
    input.trim_end_matches('/').to_string()
}

fn truncate_for_error(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        input.to_string()
    } else {
        format!("{}...", &input[..max_len])
    }
}

fn extract_usage(value: &serde_json::Value) -> Option<TokenUsage> {
    let usage = value.get("usage")?;
    let prompt_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let completion_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt_tokens.saturating_add(completion_tokens));
    Some(TokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    })
}

fn collect_text_fragments(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(s) => out.push_str(s),
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_text_fragments(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                out.push_str(text);
            }
            if let Some(text) = map.get("output_text").and_then(|v| v.as_str()) {
                out.push_str(text);
            }
            if let Some(content) = map.get("content") {
                collect_text_fragments(content, out);
            }
            if let Some(delta) = map.get("delta") {
                collect_text_fragments(delta, out);
            }
            if let Some(message) = map.get("message") {
                collect_text_fragments(message, out);
            }
        }
        _ => {}
    }
}

fn extract_openai_text(value: &serde_json::Value) -> Option<String> {
    let mut out = String::new();

    if let Some(choice) = value.get("choices").and_then(|v| v.get(0)) {
        collect_text_fragments(choice, &mut out);
        if !out.trim().is_empty() {
            return Some(out);
        }
    }

    if let Some(text) = value
        .get("choices")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
    {
        return Some(text.to_string());
    }

    if let Some(output) = value.get("output") {
        collect_text_fragments(output, &mut out);
        if !out.trim().is_empty() {
            return Some(out);
        }
    }

    if let Some(content) = value.get("content") {
        collect_text_fragments(content, &mut out);
        if !out.trim().is_empty() {
            return Some(out);
        }
    }

    if let Some(text) = value.get("output_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    None
}

fn extract_openai_error(value: &serde_json::Value) -> Option<String> {
    value
        .get("error")
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            value
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

