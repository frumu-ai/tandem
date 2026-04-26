fn build_channel_session_create_body(
    title: &str,
    security_profile: ChannelSecurityProfile,
    project_id: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "title": title,
        "permission": build_channel_session_permissions(security_profile),
    });
    if let Some(project_id) = project_id {
        payload["project_id"] = serde_json::json!(project_id);
    }
    if security_profile != ChannelSecurityProfile::PublicDemo {
        payload["directory"] = serde_json::json!(".");
    }
    payload
}

async fn refresh_channel_session_permissions(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    security_profile: ChannelSecurityProfile,
) {
    let client = reqwest::Client::new();
    let permissions = build_channel_session_permissions(security_profile);
    let response = add_auth(
        client.patch(format!("{base_url}/session/{session_id}")),
        api_token,
    )
    .json(&serde_json::json!({ "permission": permissions }))
    .send()
    .await;
    match response {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => warn!(
            "failed to refresh permissions for session '{}': HTTP {}",
            session_id,
            resp.status()
        ),
        Err(err) => warn!(
            "failed to refresh permissions for session '{}': {}",
            session_id, err
        ),
    }
}

/// Look up an existing session or create a new one via `POST /session`.
async fn get_or_create_session(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> Option<String> {
    let map_key = session_map_key(msg);
    let legacy_key = legacy_session_map_key(msg);
    {
        let mut guard = session_map.lock().await;
        if let Some(record) = guard.get_mut(&map_key) {
            record.last_seen_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let sid = record.session_id.clone();
            // Persist the updated last_seen_at_ms
            persist_session_map(&guard).await;
            drop(guard);
            refresh_channel_session_permissions(base_url, api_token, &sid, security_profile).await;
            return Some(sid);
        }
        if let Some(mut legacy_record) = guard.remove(&legacy_key) {
            legacy_record.last_seen_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            legacy_record.scope_id = Some(msg.scope.id.clone());
            legacy_record.scope_kind = Some(session_scope_kind_label(msg).to_string());
            let sid = legacy_record.session_id.clone();
            guard.insert(map_key.clone(), legacy_record);
            persist_session_map(&guard).await;
            drop(guard);
            refresh_channel_session_permissions(base_url, api_token, &sid, security_profile).await;
            return Some(sid);
        }
    }

    let client = reqwest::Client::new();
    let title = session_title_prefix(msg);
    let public_memory_project_id = if security_profile == ChannelSecurityProfile::PublicDemo {
        Some(public_channel_memory_scope_key(msg))
    } else {
        None
    };
    let body = build_channel_session_create_body(
        &title,
        security_profile,
        public_memory_project_id.as_deref(),
    );

    let resp = add_auth(client.post(format!("{base_url}/session")), api_token)
        .json(&body)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            error!("failed to create session: {e}");
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            error!("session create response parse error: {e}");
            return None;
        }
    };

    let session_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;

    let mut guard = session_map.lock().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    guard.insert(
        map_key,
        SessionRecord {
            session_id: session_id.clone(),
            created_at_ms: now,
            last_seen_at_ms: now,
            channel: msg.channel.clone(),
            sender: msg.sender.clone(),
            scope_id: Some(msg.scope.id.clone()),
            scope_kind: Some(session_scope_kind_label(msg).to_string()),
            tool_preferences: None,
            workflow_planner_session_id: None,
        },
    );
    persist_session_map(&guard).await;
    drop(guard);
    refresh_channel_session_permissions(base_url, api_token, &session_id, security_profile).await;

    Some(session_id)
}

async fn set_channel_workflow_planner_session_id(
    msg: &ChannelMessage,
    session_map: &SessionMap,
    workflow_planner_session_id: Option<String>,
) {
    let map_key = session_map_key(msg);
    let mut guard = session_map.lock().await;
    if let Some(record) = guard.get_mut(&map_key) {
        record.workflow_planner_session_id = workflow_planner_session_id;
        record.last_seen_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        persist_session_map(&guard).await;
    }
}

/// Submit a message to a Tandem session using `prompt_async` and stream
/// the result via the SSE event bus (`GET /event?sessionID=...&runID=...`).
///
/// Falls back to an error string if the initial fire fails or the stream
/// never completes within `timeout_secs`.
async fn run_in_session(
    session_id: &str,
    content: &str,
    base_url: &str,
    api_token: &str,
    attachment_path: Option<&str>,
    attachment_url: Option<&str>,
    attachment_mime: Option<&str>,
    attachment_filename: Option<&str>,
    agent: Option<&str>,
    tool_allowlist: Option<&Vec<String>>,
    channel_name: &str,
    strict_kb_grounding_override: Option<bool>,
) -> anyhow::Result<String> {
    let timeout_secs: u64 = std::env::var("TANDEM_CHANNEL_MAX_WAIT_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs + 30))
        .build()?;

    let mut parts = Vec::new();
    let attachment_source = attachment_path.or(attachment_url);
    if let (Some(source), Some(mime)) = (attachment_source, attachment_mime) {
        parts.push(serde_json::json!({
            "type": "file",
            "mime": mime,
            "filename": attachment_filename,
            "url": source
        }));
    }
    parts.push(serde_json::json!({ "type": "text", "text": content }));
    let mut body = serde_json::json!({ "parts": parts });
    if let Some(agent) = agent {
        body["agent"] = serde_json::json!(agent);
    }
    if let Some(allowlist) = tool_allowlist {
        body["tool_allowlist"] = serde_json::json!(allowlist);
    }
    let channel_runtime_config =
        fetch_channel_runtime_config(&client, base_url, api_token, channel_name)
            .await
            .unwrap_or_default();
    let model_spec = match channel_runtime_config.model.clone() {
        Some(model) => Some(model),
        None => fetch_default_model_spec(&client, base_url, api_token)
            .await
            .ok()
            .flatten(),
    };
    if let Some(model) = model_spec {
        body["model"] = model;
    }
    let strict_kb_grounding =
        strict_kb_grounding_override.unwrap_or(channel_runtime_config.strict_kb_grounding);
    if strict_kb_grounding {
        body["strict_kb_grounding"] = serde_json::json!(true);
    }

    // Request run metadata so we can bind SSE to this specific run.
    let submit_prompt = || {
        add_auth(
            client.post(format!(
                "{base_url}/session/{session_id}/prompt_async?return=run"
            )),
            api_token,
        )
        .json(&body)
    };
    let mut resp = submit_prompt().send().await?;
    if resp.status() == reqwest::StatusCode::CONFLICT {
        let conflict_text = resp.text().await.unwrap_or_default();
        let conflict_json: serde_json::Value =
            serde_json::from_str(&conflict_text).unwrap_or_default();
        let active_run_id = conflict_json
            .get("activeRun")
            .and_then(|v| v.get("runID").or_else(|| v.get("run_id")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let retry_after_ms = conflict_json
            .get("retryAfterMs")
            .and_then(|v| v.as_u64())
            .unwrap_or(500)
            .clamp(100, 5_000);
        if active_run_id.is_empty() {
            anyhow::bail!("prompt_async failed (409 Conflict): {conflict_text}");
        }
        let cancel_url = format!("{base_url}/session/{session_id}/run/{active_run_id}/cancel");
        let _ = add_auth(client.post(cancel_url), api_token)
            .json(&serde_json::json!({}))
            .send()
            .await;
        tokio::time::sleep(Duration::from_millis(retry_after_ms)).await;
        resp = submit_prompt().send().await?;
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("prompt_async failed ({status}): {err}");
    }

    // Newer engines may return 204/empty when no run payload is emitted.
    // Treat empty as "no run id" rather than surfacing a decode failure.
    let fire_text = resp.text().await?;
    let fire_json: serde_json::Value = if fire_text.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&fire_text).map_err(|e| {
            anyhow::anyhow!("prompt_async run payload parse failed: {e}: {fire_text}")
        })?
    };
    let _run_id = fire_json
        .get("runID")
        .or_else(|| fire_json.get("run_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Stream the SSE event bus until the run finishes or we timeout.
    // Run-filtered streams can miss events when engines emit session-scoped updates.
    // Subscribe by session for robust delivery in channels.
    let event_url = format!("{base_url}/event?sessionID={session_id}");

    use futures_util::StreamExt;
    let mut content_buf = String::new();
    let mut last_error: Option<String> = None;
    let mut line_buf = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut reconnect_attempts = 0usize;
    let mut body_stream = open_channel_event_stream(&client, &event_url, api_token)
        .await?
        .bytes_stream();

    'outer: loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        match tokio::time::timeout(Duration::from_secs(60), body_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                line_buf.push_str(&String::from_utf8_lossy(&chunk));
            }
            Ok(Some(Err(e))) => {
                let err_text = e.to_string();
                let recoverable =
                    should_retry_channel_event_stream(&err_text, &content_buf, deadline)
                        && reconnect_attempts < 2;
                if err_text.contains("error decoding response body") {
                    tracing::warn!(
                        "Channel SSE stream closed while reading response body: {err_text}"
                    );
                } else {
                    tracing::warn!("Channel SSE stream error: {err_text}");
                }
                if recoverable {
                    reconnect_attempts += 1;
                    tokio::time::sleep(Duration::from_millis(250 * reconnect_attempts as u64))
                        .await;
                    match open_channel_event_stream(&client, &event_url, api_token).await {
                        Ok(resp) => {
                            body_stream = resp.bytes_stream();
                            continue 'outer;
                        }
                        Err(err) => {
                            last_error = Some(err.to_string());
                        }
                    }
                } else if !err_text.trim().is_empty() {
                    last_error = Some(err_text);
                }
                break 'outer;
            }
            Ok(None) => {
                if should_retry_channel_event_stream("eof", &content_buf, deadline)
                    && reconnect_attempts < 2
                {
                    reconnect_attempts += 1;
                    tokio::time::sleep(Duration::from_millis(250 * reconnect_attempts as u64))
                        .await;
                    match open_channel_event_stream(&client, &event_url, api_token).await {
                        Ok(resp) => {
                            body_stream = resp.bytes_stream();
                            continue 'outer;
                        }
                        Err(err) => {
                            last_error = Some(err.to_string());
                        }
                    }
                }
                break 'outer;
            }
            Err(_) => {
                if should_retry_channel_event_stream("timeout", &content_buf, deadline)
                    && reconnect_attempts < 2
                {
                    reconnect_attempts += 1;
                    tokio::time::sleep(Duration::from_millis(250 * reconnect_attempts as u64))
                        .await;
                    match open_channel_event_stream(&client, &event_url, api_token).await {
                        Ok(resp) => {
                            body_stream = resp.bytes_stream();
                            continue 'outer;
                        }
                        Err(err) => {
                            last_error = Some(err.to_string());
                        }
                    }
                } else {
                    last_error = Some(
                        "channel event stream timed out while waiting for updates".to_string(),
                    );
                }
                break 'outer;
            }
        }

        // Process complete SSE lines
        while let Some(pos) = line_buf.find('\n') {
            let raw = line_buf[..pos].trim_end_matches('\r').to_string();
            line_buf = line_buf[pos + 1..].to_string();

            let data = raw.strip_prefix("data:").map(str::trim);
            let Some(data) = data else { continue };
            if data == "[DONE]" {
                break 'outer;
            }

            let Ok(evt) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };

            let event_type = evt
                .get("type")
                .or_else(|| evt.get("event"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if event_type == "message.part.updated" {
                if let Some(props) = evt.get("properties") {
                    let is_text = props
                        .get("part")
                        .and_then(|p| p.get("type"))
                        .and_then(|v| v.as_str())
                        .map(|v| v == "text")
                        .unwrap_or(false);
                    if is_text {
                        if let Some(delta) = props.get("delta").and_then(|v| v.as_str()) {
                            content_buf.push_str(delta);
                        }
                    }
                }
                continue;
            }

            if event_type == "session.error" {
                if let Some(message) = extract_event_error_message(&evt) {
                    last_error = Some(message);
                }
                continue;
            }

            match event_type {
                "session.message.delta" | "content" => {
                    if let Some(delta) = evt
                        .get("delta")
                        .and_then(|v| v.as_str())
                        .or_else(|| evt.get("text").and_then(|v| v.as_str()))
                    {
                        content_buf.push_str(delta);
                    }
                }
                "session.run.finished"
                | "session.run.completed"
                | "session.run.failed"
                | "session.run.cancelled"
                | "session.run.canceled"
                | "done" => {
                    if let Some(err) = extract_event_error_message(&evt) {
                        last_error = Some(err);
                    }
                    break 'outer;
                }
                _ => {}
            }
        }
    }

    if strict_kb_grounding {
        // Fast runs may complete before we attach SSE, and persisted assistant
        // messages can lag slightly behind run completion. Retry briefly.
        for _ in 0..20 {
            if let Ok(Some(fallback)) =
                fetch_latest_assistant_message(&client, base_url, api_token, session_id).await
            {
                return Ok(fallback);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if let Some(error_message) = last_error {
            return Ok(format!(
                "⚠️ Error: {}",
                truncate_for_channel(&error_message, 320)
            ));
        }
        return Ok("(no response)".to_string());
    }

    if content_buf.is_empty() {
        // Fast runs may complete before we attach SSE, and persisted assistant
        // messages can lag slightly behind run completion. Retry briefly.
        for _ in 0..20 {
            if let Ok(Some(fallback)) =
                fetch_latest_assistant_message(&client, base_url, api_token, session_id).await
            {
                return Ok(fallback);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if let Some(error_message) = last_error {
            return Ok(format!(
                "⚠️ Error: {}",
                truncate_for_channel(&error_message, 320)
            ));
        }
        return Ok("(no response)".to_string());
    }

    Ok(content_buf)
}

async fn open_channel_event_stream(
    client: &reqwest::Client,
    event_url: &str,
    api_token: &str,
) -> anyhow::Result<reqwest::Response> {
    let resp = add_auth(client.get(event_url), api_token)
        .header("Accept", "text/event-stream")
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("event stream request failed ({status}): {err}");
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !content_type
        .to_ascii_lowercase()
        .contains("text/event-stream")
    {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "event stream returned unexpected content-type '{}' ({status}): {}",
            content_type,
            truncate_for_channel(&body, 400)
        );
    }
    Ok(resp)
}

fn should_retry_channel_event_stream(
    reason: &str,
    content_buf: &str,
    deadline: tokio::time::Instant,
) -> bool {
    let before_deadline = tokio::time::Instant::now() < deadline;
    let empty_content = content_buf.trim().is_empty();
    empty_content
        && before_deadline
        && (matches!(reason, "eof" | "timeout") || reason.contains("error decoding response body"))
}

fn truncate_for_channel(input: &str, max_chars: usize) -> String {
    let mut out = input.trim().chars().take(max_chars).collect::<String>();
    if input.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn extract_event_error_message(evt: &serde_json::Value) -> Option<String> {
    let paths = [
        evt.get("error").and_then(|e| e.get("message")),
        evt.get("error"),
        evt.get("message"),
        evt.get("properties")
            .and_then(|p| p.get("error"))
            .and_then(|e| e.get("message")),
        evt.get("properties").and_then(|p| p.get("error")),
        evt.get("properties").and_then(|p| p.get("message")),
    ];

    for value in paths.into_iter().flatten() {
        if let Some(text) = value.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
            continue;
        }
        if let Some(obj) = value.as_object() {
            if let Some(text) = obj.get("message").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

async fn fetch_default_model_spec(
    client: &reqwest::Client,
    base_url: &str,
    api_token: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let url = format!("{base_url}/config/providers");
    let resp = add_auth(client.get(&url), api_token).send().await?;
    if !resp.status().is_success() {
        return Ok(None);
    }

    let cfg: serde_json::Value = resp.json().await?;
    let default_provider = cfg
        .get("default")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if default_provider.is_empty() {
        return Ok(None);
    }

    let default_model = cfg
        .get("providers")
        .and_then(|v| v.get(default_provider))
        .and_then(|v| v.get("default_model").or_else(|| v.get("defaultModel")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if default_model.is_empty() {
        return Ok(None);
    }

    Ok(Some(serde_json::json!({
        "provider_id": default_provider,
        "model_id": default_model
    })))
}

#[derive(Debug, Clone, Default)]
struct ChannelRuntimeConfig {
    model: Option<serde_json::Value>,
    strict_kb_grounding: bool,
}

async fn fetch_channel_runtime_config(
    client: &reqwest::Client,
    base_url: &str,
    api_token: &str,
    channel_name: &str,
) -> anyhow::Result<ChannelRuntimeConfig> {
    let channel_name = channel_name.trim().to_ascii_lowercase();
    if channel_name.is_empty() {
        return Ok(ChannelRuntimeConfig::default());
    }

    let url = format!("{base_url}/channels/config");
    let resp = add_auth(client.get(&url), api_token).send().await?;
    if !resp.status().is_success() {
        return Ok(ChannelRuntimeConfig::default());
    }

    let cfg: serde_json::Value = resp.json().await?;
    let Some(channel_cfg) = cfg.get(&channel_name) else {
        return Ok(ChannelRuntimeConfig::default());
    };

    let provider_id = channel_cfg
        .get("model_provider_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let model_id = channel_cfg
        .get("model_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let model = if provider_id.is_empty() || model_id.is_empty() {
        None
    } else {
        Some(serde_json::json!({
            "provider_id": provider_id,
            "model_id": model_id
        }))
    };
    Ok(ChannelRuntimeConfig {
        model,
        strict_kb_grounding: channel_cfg
            .get("strict_kb_grounding")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

/// Fallback for channel delivery: if the SSE stream did not emit text deltas,
/// fetch persisted session history and return the latest assistant text.
async fn fetch_latest_assistant_message(
    client: &reqwest::Client,
    base_url: &str,
    api_token: &str,
    session_id: &str,
) -> anyhow::Result<Option<String>> {
    let url = format!("{base_url}/session/{session_id}/message");
    let resp = add_auth(client.get(&url), api_token).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("session message fallback failed ({status}): {err}");
    }

    let messages: serde_json::Value = resp.json().await?;
    let Some(items) = messages.as_array() else {
        return Ok(None);
    };

    for msg in items.iter().rev() {
        let role = msg
            .get("info")
            .and_then(|info| info.get("role"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if role != "assistant" {
            continue;
        }

        let Some(parts) = msg.get("parts").and_then(|v| v.as_array()) else {
            continue;
        };

        let mut text = String::new();
        for part in parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if part_type == "text" || part_type == "reasoning" || part_type.is_empty() {
                if let Some(chunk) = part.get("text").and_then(|v| v.as_str()) {
                    if !chunk.trim().is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(chunk);
                    }
                }
            }
        }

        if !text.trim().is_empty() {
            return Ok(Some(text));
        }
    }

    Ok(None)
}

/// Send an approve or deny decision to the tandem-server tool approval endpoint.
/// Path: POST /sessions/{session_id}/tools/{tool_call_id}/approve|deny
async fn relay_tool_decision(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    tool_call_id: &str,
    approved: bool,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let action = if approved { "approve" } else { "deny" };
    let url = format!("{base_url}/sessions/{session_id}/tools/{tool_call_id}/{action}");
    let resp = add_auth(client.post(&url), api_token).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("relay_tool_decision failed ({status})");
    }
    Ok(())
}

/// Fetch the cross-subsystem pending-approvals list from the engine.
/// Used by `/pending` to render a chat-friendly summary of outstanding gates.
async fn fetch_pending_approvals(
    base_url: &str,
    api_token: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let url = format!("{base_url}/approvals/pending");
    let resp = add_auth(client.get(&url), api_token).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("fetch_pending_approvals failed ({status})");
    }
    let body: serde_json::Value = resp.json().await?;
    Ok(body
        .get("approvals")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default())
}

/// Send a gate-decide decision (`approve` / `rework` / `cancel`) for an
/// `automation_v2` workflow run. Used by `/rework` and (eventually)
/// contextual `/approve` / `/reject` slash commands.
///
/// Path: `POST /automations/v2/runs/{run_id}/gate`. Reuses the same
/// authoritative subsystem handler the inbox UI and channel cards already
/// dispatch through, so audit semantics and the W2.6 race UX (winner
/// identity in 409) come along for free.
async fn relay_gate_decision(
    base_url: &str,
    api_token: &str,
    run_id: &str,
    decision: &str,
    reason: Option<&str>,
) -> anyhow::Result<reqwest::StatusCode> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let url = format!("{base_url}/automations/v2/runs/{run_id}/gate");
    let body = match reason {
        Some(text) => serde_json::json!({ "decision": decision, "reason": text }),
        None => serde_json::json!({ "decision": decision }),
    };
    let resp = add_auth(client.post(&url), api_token)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 409 {
        // Surface the error body when the engine rejected the call for a
        // reason other than the documented race conflict.
        let detail = resp.text().await.unwrap_or_default();
        anyhow::bail!("relay_gate_decision failed ({status}): {detail}");
    }
    Ok(status)
}

/// Render `/pending` output: a compact list of outstanding approvals with
/// just enough info for the user to pick one out (workflow name, run_id,
/// action_kind, requested-at).
fn render_pending_text(approvals: &[serde_json::Value]) -> String {
    if approvals.is_empty() {
        return "✅ No approvals waiting.".to_string();
    }
    let mut lines = vec![format!(
        "*{} pending approval{}*",
        approvals.len(),
        if approvals.len() == 1 { "" } else { "s" }
    )];
    for (i, request) in approvals.iter().take(20).enumerate() {
        let workflow = request
            .get("workflow_name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        let run_id = request
            .get("run_id")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let action = request
            .get("action_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let action_suffix = if action.is_empty() {
            String::new()
        } else {
            format!(" — {action}")
        };
        lines.push(format!("{}. {workflow} `{run_id}`{action_suffix}", i + 1));
    }
    if approvals.len() > 20 {
        lines.push(format!("…and {} more.", approvals.len() - 20));
    }
    lines.push(String::new());
    lines
        .push("Decide via the buttons on each card, or `/rework <run_id> <feedback>`.".to_string());
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Slash command handler dispatch
// ---------------------------------------------------------------------------

async fn handle_slash_command(
    cmd: SlashCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    if let Some(reason) = blocked_command_reason(&cmd, security_profile) {
        return format!(
            "🔒 This command is disabled in this channel for security.\n{}\nUse `/help` to see which Tandem capabilities are available here versus disabled for this public integration.",
            reason
        );
    }
    match cmd {
        SlashCommand::Help { topic } => help_text(topic.as_deref(), security_profile),
        SlashCommand::ListSessions => list_sessions_text(msg, base_url, api_token).await,
        SlashCommand::New { name } => {
            new_session_text(
                name,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        SlashCommand::Resume { query } => {
            resume_session_text(query, msg, base_url, api_token, session_map).await
        }
        SlashCommand::Status => status_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Run => run_status_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Cancel => cancel_run_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Todos => todos_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Requests => requests_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Answer {
            question_id,
            answer,
        } => answer_question_text(question_id, answer, msg, base_url, api_token, session_map).await,
        SlashCommand::Providers => providers_text(base_url, api_token).await,
        SlashCommand::Models { provider } => models_text(provider, base_url, api_token).await,
        SlashCommand::Model { model_id } => set_model_text(model_id, base_url, api_token).await,
        SlashCommand::Rename { name } => {
            rename_session_text(name, msg, base_url, api_token, session_map).await
        }
        SlashCommand::Approve { tool_call_id } => {
            let session_id = active_session_id(msg, session_map).await;
            match session_id {
                None => "⚠️ No active session — nothing to approve.".to_string(),
                Some(sid) => {
                    match relay_tool_decision(base_url, api_token, &sid, &tool_call_id, true).await
                    {
                        Ok(()) => format!("✅ Approved tool call `{tool_call_id}`."),
                        Err(e) => format!("⚠️ Could not approve: {e}"),
                    }
                }
            }
        }
        SlashCommand::Deny { tool_call_id } => {
            let session_id = active_session_id(msg, session_map).await;
            match session_id {
                None => "⚠️ No active session — nothing to deny.".to_string(),
                Some(sid) => {
                    match relay_tool_decision(base_url, api_token, &sid, &tool_call_id, false).await
                    {
                        Ok(()) => format!("🚫 Denied tool call `{tool_call_id}`."),
                        Err(e) => format!("⚠️ Could not deny: {e}"),
                    }
                }
            }
        }
        SlashCommand::Pending => {
            // Note: the engine endpoint already filters by tenant from
            // request context. Per-channel filtering (e.g. only show this
            // channel's runs) is a future refinement that needs the surface
            // user → engine principal resolver wired into the request middleware.
            match fetch_pending_approvals(base_url, api_token).await {
                Ok(approvals) => render_pending_text(&approvals),
                Err(e) => format!("⚠️ Could not fetch pending approvals: {e}"),
            }
        }
        SlashCommand::Rework { run_id, feedback } => {
            match relay_gate_decision(base_url, api_token, &run_id, "rework", Some(&feedback)).await
            {
                Ok(status) if status.is_success() => {
                    format!("↻ Sent run `{run_id}` back for rework with your feedback.")
                }
                Ok(status) if status.as_u16() == 409 => {
                    // Race: another surface decided this gate first. The
                    // 409 body carries the winner's identity (W2.6) but we
                    // do not parse it here for v1 — the toast is enough.
                    format!("⚠️ Run `{run_id}` was already decided by another operator.")
                }
                Ok(status) => format!("⚠️ Rework rejected by engine ({status})."),
                Err(e) => format!("⚠️ Could not send rework: {e}"),
            }
        }
        SlashCommand::Schedule { action } => {
            schedule_command_text(action, msg, base_url, api_token, session_map).await
        }
        SlashCommand::Automations { action } => {
            automations_command_text(action, base_url, api_token).await
        }
        SlashCommand::Runs { action } => runs_command_text(action, base_url, api_token).await,
        SlashCommand::Memory { action } => {
            memory_command_text(
                action,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        SlashCommand::Workspace { action } => {
            workspace_command_text(action, msg, base_url, api_token, session_map).await
        }
        SlashCommand::Tools { action } => {
            tools_command_text(action, msg, base_url, api_token, security_profile).await
        }
        SlashCommand::Mcp { action } => mcp_command_text(action, msg, base_url, api_token).await,
        SlashCommand::Packs { action } => packs_command_text(action, base_url, api_token).await,
        SlashCommand::Config { action } => config_command_text(action, base_url, api_token).await,
    }
}

fn blocked_command_reason(
    cmd: &SlashCommand,
    security_profile: ChannelSecurityProfile,
) -> Option<&'static str> {
    if security_profile != ChannelSecurityProfile::PublicDemo {
        return None;
    }
    let command_name = slash_command_name(cmd);
    let Some(capability) = command_capability(command_name) else {
        return None;
    };
    if capability.enabled_for(security_profile) {
        None
    } else {
        capability.public_demo_reason
    }
}

fn slash_command_name(cmd: &SlashCommand) -> &'static str {
    match cmd {
        SlashCommand::New { .. } => "new",
        SlashCommand::ListSessions => "sessions",
        SlashCommand::Resume { .. } => "resume",
        SlashCommand::Rename { .. } => "rename",
        SlashCommand::Status => "status",
        SlashCommand::Run => "run",
        SlashCommand::Cancel => "cancel",
        SlashCommand::Todos => "todos",
        SlashCommand::Requests => "requests",
        SlashCommand::Answer { .. } => "answer",
        SlashCommand::Providers => "providers",
        SlashCommand::Models { .. } => "models",
        SlashCommand::Model { .. } => "model",
        SlashCommand::Help { .. } => "help",
        SlashCommand::Approve { .. } => "approve",
        SlashCommand::Deny { .. } => "deny",
        SlashCommand::Pending => "pending",
        SlashCommand::Rework { .. } => "rework",
        SlashCommand::Schedule { .. } => "schedule",
        SlashCommand::Automations { .. } => "automations",
        SlashCommand::Runs { .. } => "runs",
        SlashCommand::Memory { .. } => "memory",
        SlashCommand::Workspace { .. } => "workspace",
        SlashCommand::Tools { .. } => "tools",
        SlashCommand::Mcp { .. } => "mcp",
        SlashCommand::Packs { .. } => "packs",
        SlashCommand::Config { .. } => "config",
    }
}

// ---------------------------------------------------------------------------
// Individual slash command implementations
// ---------------------------------------------------------------------------

fn help_text(topic: Option<&str>, security_profile: ChannelSecurityProfile) -> String {
    match topic.map(|value| value.trim().to_ascii_lowercase()) {
        Some(topic) if topic == "schedule" || topic == "workflow" || topic == "automation" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "schedule",
                    "Workflow planning and automation setup are disabled in this public channel for security.",
                )
            } else {
                schedule_help_text()
            }
        }
        Some(topic) if topic == "automations" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "automations",
                    "Automation control commands are disabled in this public channel for security.",
                )
            } else {
                automations_help_text()
            }
        }
        Some(topic) if topic == "runs" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "runs",
                    "Run control commands are disabled in this public channel for security.",
                )
            } else {
                runs_help_text()
            }
        }
        Some(topic) if topic == "memory" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                public_demo_memory_help_text()
            } else {
                memory_help_text()
            }
        }
        Some(topic) if topic == "workspace" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "workspace",
                    "Workspace and file access commands are disabled in this public channel for security.",
                )
            } else {
                workspace_help_text()
            }
        }
        Some(topic) if topic == "tools" => tools_help_text(security_profile),
        Some(topic) if topic == "mcp" => mcp_help_text(security_profile),
        Some(topic) if topic == "packs" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "packs",
                    "Pack install and inspection commands are disabled in this public channel for security.",
                )
            } else {
                packs_help_text()
            }
        }
        Some(topic) if topic == "config" => config_help_text(security_profile),
        Some(topic) => format!(
            "⚠️ Unknown help topic `{topic}`.\nUse `/help` to list command groups or `/help automations`, `/help memory`, `/help workspace`, `/help mcp`, `/help packs`, `/help config`, or `/help schedule`."
        ),
        None => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                public_demo_help_text()
            } else {
                registry_driven_help_text(security_profile)
            }
        }
    }
}

fn registry_driven_help_text(security_profile: ChannelSecurityProfile) -> String {
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<&'static str, Vec<&'static ChannelCommandCapability>> =
        BTreeMap::new();
    for capability in slash_command_capabilities()
        .iter()
        .filter(|capability| capability.enabled_for(security_profile))
    {
        groups
            .entry(capability.audience)
            .or_default()
            .push(capability);
    }

    let mut lines = vec!["🤖 *Tandem Commands*".to_string()];
    for (audience, commands) in groups {
        let heading = match audience {
            "session" => "Core session commands",
            "approval" => "Session ops",
            "model" => "Model controls",
            "automation" => "Workflow planning and automation",
            "operator" => "Operator commands",
            "meta" => "Help",
            _ => "Commands",
        };
        if !commands.is_empty() {
            lines.push(format!("{heading}:"));
            for capability in commands {
                let command = if capability.args.is_empty() {
                    format!("/{name}", name = capability.name)
                } else {
                    format!(
                        "/{name} {args}",
                        name = capability.name,
                        args = capability.args
                    )
                };
                lines.push(format!("{} — {}", command, capability.description));
            }
            lines.push(String::new());
        }
    }
    let trailing_empty = lines.last().map(|line| line.is_empty()).unwrap_or(false);
    if trailing_empty {
        lines.pop();
    }
    lines.join("\n")
}

fn disabled_help_text(topic: &str, reason: &str) -> String {
    format!(
        "🔒 *{topic} commands are disabled in this channel*\n{reason}\n\nThis Tandem integration supports those capabilities in trusted/operator channels, but they are intentionally blocked here."
    )
}

fn public_demo_help_text() -> String {
    "🤖 *Tandem Public Demo Commands*\n\
Available here:\n\
/new [name] — start a fresh session\n\
/sessions — list your recent sessions\n\
/resume <id or name> — switch to a previous session\n\
/rename <name> — rename the current session\n\
/status — show current session info\n\
/run — show active run state\n\
/cancel — cancel the active run\n\
/memory — search and store channel-scoped public memory\n\
/help — show this message\n\
\n\
Disabled in this public channel for security:\n\
/providers, /models, /model — runtime and model reconfiguration\n\
/workspace — file and repo access\n\
/mcp — external connector access\n\
/tools — tool-scope override controls\n\
/config — runtime configuration access\n\
/schedule, /automations, /runs — operator workflow control\n\
/packs — pack install and inspection controls\n\
\n\
These are real Tandem capabilities, but this integration is intentionally hardened so you can explore it safely in public."
        .to_string()
}

fn schedule_help_text() -> String {
    "🗓️ *Workflow Planning Commands*\n\
/schedule help — show this guide\n\
/schedule plan <prompt> — create a workflow draft from a plain-English goal\n\
/schedule show <plan_id> — inspect the current draft\n\
/schedule edit <plan_id> <message> — revise the draft conversationally\n\
/schedule reset <plan_id> — reset the draft back to its initial preview\n\
/schedule apply <plan_id> — turn the draft into a saved automation\n\
\n\
Examples:\n\
/schedule plan Every weekday at 9am summarize GitHub notifications and email me the blockers\n\
/schedule edit wfplan-123 change this to every Monday and Friday at 8am\n\
/schedule apply wfplan-123\n\
\n\
Tip: `/schedule plan` uses the current session workspace when available so the planner can target the right repo."
        .to_string()
}
