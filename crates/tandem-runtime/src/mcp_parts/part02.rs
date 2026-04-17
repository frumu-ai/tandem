fn resolve_secret_ref_value(
    secret_ref: &McpSecretRef,
    current_tenant: &TenantContext,
) -> Option<String> {
    match secret_ref {
        McpSecretRef::Store {
            secret_id,
            tenant_context,
        } => {
            let secret_ref = SecretRef {
                org_id: tenant_context.org_id.clone(),
                workspace_id: tenant_context.workspace_id.clone(),
                provider: "mcp_header".to_string(),
                secret_id: secret_id.trim().to_string(),
                name: secret_id.trim().to_string(),
            };
            if secret_ref.validate_for_tenant(current_tenant).is_err() {
                return None;
            }
            tandem_core::load_provider_auth()
                .get(&secret_id.trim().to_ascii_lowercase())
                .cloned()
                .filter(|value| !value.trim().is_empty())
        }
        McpSecretRef::Env { env } => std::env::var(env)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        McpSecretRef::BearerEnv { env } => std::env::var(env)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| format!("Bearer {value}")),
    }
}

fn local_tenant_context() -> TenantContext {
    LocalImplicitTenant.into()
}

fn parse_secret_header_reference(raw: &str) -> Option<McpSecretRef> {
    let trimmed = raw.trim();
    if let Some(env) = trimmed
        .strip_prefix("${env:")
        .and_then(|rest| rest.strip_suffix('}'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(McpSecretRef::Env {
            env: env.to_string(),
        });
    }
    if let Some(env) = trimmed
        .strip_prefix("${bearer_env:")
        .and_then(|rest| rest.strip_suffix('}'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(McpSecretRef::BearerEnv {
            env: env.to_string(),
        });
    }
    if let Some(env) = trimmed
        .strip_prefix("Bearer ${env:")
        .and_then(|rest| rest.strip_suffix("}"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(McpSecretRef::BearerEnv {
            env: env.to_string(),
        });
    }
    None
}

fn header_name_is_sensitive(header_name: &str) -> bool {
    let normalized = header_name.trim().to_ascii_lowercase();
    normalized == "authorization"
        || normalized == "proxy-authorization"
        || normalized == "x-api-key"
        || normalized.contains("token")
        || normalized.contains("secret")
        || normalized.ends_with("api-key")
        || normalized.ends_with("api_key")
}

fn mcp_header_secret_id(server_name: &str, header_name: &str) -> String {
    format!(
        "mcp_header::{}::{}",
        sanitize_namespace_segment(server_name),
        sanitize_namespace_segment(header_name)
    )
}

fn parse_stdio_transport(transport: &str) -> Option<&str> {
    transport.strip_prefix("stdio:").map(str::trim)
}

fn parse_remote_endpoint(transport: &str) -> Option<String> {
    let trimmed = transport.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }
    for prefix in ["http:", "https:"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let endpoint = rest.trim();
            if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
                return Some(endpoint.to_string());
            }
        }
    }
    None
}

fn server_tool_rows(server: &McpServer) -> Vec<McpRemoteTool> {
    let server_slug = sanitize_namespace_segment(&server.name);
    server
        .tool_cache
        .iter()
        .map(|tool| {
            let tool_slug = sanitize_namespace_segment(&tool.tool_name);
            McpRemoteTool {
                server_name: server.name.clone(),
                tool_name: tool.tool_name.clone(),
                namespaced_name: format!("mcp.{server_slug}.{tool_slug}"),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                fetched_at_ms: tool.fetched_at_ms,
                schema_hash: tool.schema_hash.clone(),
            }
        })
        .collect()
}

fn sanitize_namespace_segment(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "tool".to_string()
    } else {
        cleaned.to_string()
    }
}

fn schema_hash(schema: &Value) -> String {
    let payload = serde_json::to_vec(schema).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(payload);
    format!("{:x}", hasher.finalize())
}

fn extract_auth_challenge(result: &Value, tool_name: &str) -> Option<McpAuthChallenge> {
    let authorization_url = find_string_with_priority(
        result,
        &[
            &["structuredContent", "authorization_url"],
            &["structuredContent", "authorizationUrl"],
            &["authorization_url"],
            &["authorizationUrl"],
            &["auth_url"],
        ],
        &["authorization_url", "authorizationUrl", "auth_url"],
    )?;
    let raw_message = find_string_with_priority(
        result,
        &[
            &["structuredContent", "message"],
            &["message"],
            &["structuredContent", "text"],
            &["text"],
            &["llm_instructions"],
        ],
        &["message", "text", "llm_instructions"],
    )
    .unwrap_or_else(|| "This tool requires authorization before it can run.".to_string());
    let message = sanitize_auth_message(&raw_message);
    let challenge_id = stable_id_seed(&format!("{tool_name}:{authorization_url}"));
    Some(McpAuthChallenge {
        challenge_id,
        tool_name: tool_name.to_string(),
        authorization_url,
        message,
        requested_at_ms: now_ms(),
        status: "pending".to_string(),
    })
}

fn find_string_by_any_key(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(s) = map.get(*key).and_then(|v| v.as_str()) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for child in map.values() {
                if let Some(found) = find_string_by_any_key(child, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_string_by_any_key(item, keys)),
        _ => None,
    }
}

fn find_string_with_priority(
    value: &Value,
    paths: &[&[&str]],
    fallback_keys: &[&str],
) -> Option<String> {
    for path in paths {
        if let Some(found) = find_string_at_path(value, path) {
            return Some(found);
        }
    }
    find_string_by_any_key(value, fallback_keys)
}

fn find_string_at_path(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    let s = current.as_str()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn sanitize_auth_message(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "This tool requires authorization before it can run.".to_string();
    }
    if let Some((head, _)) = trimmed.split_once("Authorize here:") {
        let head = head.trim();
        if !head.is_empty() {
            return truncate_text(head, 280);
        }
    }
    let no_newlines = trimmed.replace(['\r', '\n'], " ");
    truncate_text(no_newlines.trim(), 280)
}

fn truncate_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let truncated = input.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}

fn stable_id_seed(seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let encoded = format!("{:x}", hasher.finalize());
    encoded.chars().take(16).collect()
}

fn canonical_tool_key(tool_name: &str) -> String {
    tool_name.trim().to_ascii_lowercase()
}

fn pending_auth_from_challenge(challenge: &McpAuthChallenge) -> PendingMcpAuth {
    PendingMcpAuth {
        challenge_id: challenge.challenge_id.clone(),
        authorization_url: challenge.authorization_url.clone(),
        message: challenge.message.clone(),
        status: challenge.status.clone(),
        first_seen_ms: challenge.requested_at_ms,
        last_probe_ms: challenge.requested_at_ms,
    }
}

struct PendingAuthShortCircuit {
    output: String,
    mcp_auth: Value,
}

fn pending_auth_short_circuit(
    server: &McpServer,
    tool_key: &str,
    tool_name: &str,
    now_ms_value: u64,
    cooldown_ms: u64,
) -> Option<PendingAuthShortCircuit> {
    let pending = server.pending_auth_by_tool.get(tool_key)?;
    let elapsed = now_ms_value.saturating_sub(pending.last_probe_ms);
    if elapsed >= cooldown_ms {
        return None;
    }
    let retry_after_ms = cooldown_ms.saturating_sub(elapsed);
    let output = format!(
        "Authorization pending for `{}`.\n{}\n\nAuthorize here: {}\nRetry after {}s.",
        tool_name,
        pending.message,
        pending.authorization_url,
        retry_after_ms.div_ceil(1000)
    );
    Some(PendingAuthShortCircuit {
        output,
        mcp_auth: json!({
            "required": true,
            "pending": true,
            "blocked": true,
            "retryAfterMs": retry_after_ms,
            "challengeId": pending.challenge_id,
            "tool": tool_name,
            "authorizationUrl": pending.authorization_url,
            "message": pending.message,
            "status": pending.status
        }),
    })
}

fn normalize_tool_input_schema(schema: &mut Value) {
    normalize_schema_node(schema);
}

fn normalize_schema_node(node: &mut Value) {
    let Some(obj) = node.as_object_mut() else {
        return;
    };

    // Some MCP servers publish enums on non-string/object/array fields, which
    // OpenAI-compatible providers may reject (e.g. Gemini via OpenRouter).
    // Keep enum only when values are all strings and schema type is string-like.
    if let Some(enum_values) = obj.get("enum").and_then(|v| v.as_array()) {
        let all_strings = enum_values.iter().all(|v| v.is_string());
        let string_like_type = schema_type_allows_string_enum(obj.get("type"));
        if !all_strings || !string_like_type {
            obj.remove("enum");
        }
    }

    if let Some(properties) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
        for value in properties.values_mut() {
            normalize_schema_node(value);
        }
    }

    if let Some(items) = obj.get_mut("items") {
        normalize_schema_node(items);
    }

    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(array) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
            for child in array.iter_mut() {
                normalize_schema_node(child);
            }
        }
    }

    if let Some(additional) = obj.get_mut("additionalProperties") {
        normalize_schema_node(additional);
    }
}

fn schema_type_allows_string_enum(schema_type: Option<&Value>) -> bool {
    let Some(schema_type) = schema_type else {
        // No explicit type: keep enum to avoid over-normalizing loosely-typed schemas.
        return true;
    };

    if let Some(kind) = schema_type.as_str() {
        return kind == "string";
    }

    if let Some(kinds) = schema_type.as_array() {
        let mut saw_string = false;
        for kind in kinds {
            let Some(kind) = kind.as_str() else {
                return false;
            };
            if kind == "string" {
                saw_string = true;
                continue;
            }
            if kind != "null" {
                return false;
            }
        }
        return saw_string;
    }

    false
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn build_headers(headers: &HashMap<String, String>) -> Result<HeaderMap, String> {
    let mut map = HeaderMap::new();
    map.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    map.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    for (key, value) in headers {
        let name = HeaderName::from_bytes(key.trim().as_bytes())
            .map_err(|e| format!("Invalid header name '{key}': {e}"))?;
        let header = HeaderValue::from_str(value.trim())
            .map_err(|e| format!("Invalid header value for '{key}': {e}"))?;
        map.insert(name, header);
    }
    Ok(map)
}

async fn post_json_rpc_with_session(
    endpoint: &str,
    headers: &HashMap<String, String>,
    request: Value,
    session_id: Option<&str>,
) -> Result<(Value, Option<String>), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
    let mut req = client.post(endpoint).headers(build_headers(headers)?);
    if let Some(id) = session_id {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            req = req.header("Mcp-Session-Id", trimmed);
        }
    }
    let response = req
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("MCP request failed: {e}"))?;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let response_session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let status = response.status();
    let payload = response
        .text()
        .await
        .map_err(|e| format!("Failed to read MCP response: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "MCP endpoint returned HTTP {}: {}",
            status.as_u16(),
            payload.chars().take(400).collect::<String>()
        ));
    }

    let value = if content_type.starts_with("text/event-stream") {
        parse_sse_first_event_json(&payload).map_err(|e| {
            format!(
                "Invalid MCP SSE JSON response: {} (snippet: {})",
                e,
                payload.chars().take(400).collect::<String>()
            )
        })?
    } else if let Ok(value) = serde_json::from_str::<Value>(&payload) {
        value
    } else if let Ok(value) = parse_sse_first_event_json(&payload) {
        // Some MCP servers return SSE payloads without setting text/event-stream.
        value
    } else {
        return Err(format!(
            "Invalid MCP JSON response: {}",
            payload.chars().take(400).collect::<String>()
        ));
    };

    Ok((value, response_session_id))
}

fn parse_sse_first_event_json(payload: &str) -> Result<Value, String> {
    let mut data_lines: Vec<&str> = Vec::new();
    for raw in payload.lines() {
        let line = raw.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        }
        if line.is_empty() {
            if !data_lines.is_empty() {
                break;
            }
            continue;
        }
    }
    if data_lines.is_empty() {
        return Err("no SSE data event found".to_string());
    }
    let joined = data_lines.join("\n");
    serde_json::from_str::<Value>(&joined).map_err(|e| e.to_string())
}

fn render_mcp_content(value: &Value) -> String {
    let Some(items) = value.as_array() else {
        return value.to_string();
    };
    let mut chunks = Vec::new();
    for item in items {
        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
            chunks.push(text.to_string());
            continue;
        }
        chunks.push(item.to_string());
    }
    if chunks.is_empty() {
        value.to_string()
    } else {
        chunks.join("\n")
    }
}

fn normalize_mcp_tool_args(server: &McpServer, tool_name: &str, raw_args: Value) -> Value {
    let Some(schema) = server
        .tool_cache
        .iter()
        .find(|row| row.tool_name.eq_ignore_ascii_case(tool_name))
        .map(|row| &row.input_schema)
    else {
        return raw_args;
    };

    let mut args_obj = match raw_args {
        Value::Object(obj) => obj,
        other => return other,
    };

    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if properties.is_empty() {
        return Value::Object(args_obj);
    }

    // Build a normalized-key lookup so taskTitle -> task_title and list-id -> list_id resolve.
    let mut normalized_existing: HashMap<String, String> = HashMap::new();
    for key in args_obj.keys() {
        normalized_existing.insert(normalize_arg_key(key), key.clone());
    }

    // Copy values from normalized aliases to canonical schema property names.
    let canonical_keys = properties.keys().cloned().collect::<Vec<_>>();
    for canonical in &canonical_keys {
        if args_obj.contains_key(canonical) {
            continue;
        }
        if let Some(existing_key) = normalized_existing.get(&normalize_arg_key(canonical)) {
            if let Some(value) = args_obj.get(existing_key).cloned() {
                args_obj.insert(canonical.clone(), value);
            }
        }
    }

    // Fill required fields using conservative aliases when models choose common alternatives.
    let required = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for required_key in required {
        if args_obj.contains_key(&required_key) {
            continue;
        }
        if let Some(alias_value) = find_required_alias_value(&required_key, &args_obj) {
            args_obj.insert(required_key, alias_value);
        }
    }

    Value::Object(args_obj)
}

fn find_required_alias_value(
    required_key: &str,
    args_obj: &serde_json::Map<String, Value>,
) -> Option<Value> {
    let mut alias_candidates = vec![
        required_key.to_string(),
        required_key.to_ascii_lowercase(),
        required_key.replace('_', ""),
    ];

    // Common fallback for fields like task_title where models often send `name`.
    if required_key.contains("title") {
        alias_candidates.extend([
            "name".to_string(),
            "title".to_string(),
            "task_name".to_string(),
            "taskname".to_string(),
        ]);
    }

    // Common fallback for *_id fields where models emit `<base>` or `<base>Id`.
    if let Some(base) = required_key.strip_suffix("_id") {
        alias_candidates.extend([base.to_string(), format!("{base}id"), format!("{base}_id")]);
    }

    let mut by_normalized: HashMap<String, &Value> = HashMap::new();
    for (key, value) in args_obj {
        by_normalized.insert(normalize_arg_key(key), value);
    }

    alias_candidates
        .into_iter()
        .find_map(|candidate| by_normalized.get(&normalize_arg_key(&candidate)).cloned())
        .cloned()
}

fn normalize_arg_key(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

async fn spawn_stdio_process(command_text: &str) -> Result<Child, String> {
    if command_text.is_empty() {
        return Err("Missing stdio command".to_string());
    }
    #[cfg(windows)]
    let mut command = {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", command_text]);
        cmd
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut cmd = Command::new("sh");
        cmd.args(["-lc", command_text]);
        cmd
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.spawn().map_err(|e| e.to_string())
}

