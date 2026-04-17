fn parse_pack_builder_reply_command(content: &str) -> Option<PackBuilderReplyCommand> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "ok" | "okay"
            | "yes"
            | "y"
            | "confirm"
            | "confirmed"
            | "approve"
            | "approved"
            | "go"
            | "go ahead"
            | "proceed"
            | "do it"
            | "run it"
            | "apply"
    ) {
        return Some(PackBuilderReplyCommand::Confirm);
    }
    if matches!(lower.as_str(), "cancel" | "stop" | "abort") {
        return Some(PackBuilderReplyCommand::Cancel);
    }
    if let Some(rest) = trimmed
        .to_ascii_lowercase()
        .strip_prefix("use connectors:")
        .map(ToString::to_string)
    {
        let connectors = rest
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if !connectors.is_empty() {
            return Some(PackBuilderReplyCommand::UseConnectors(connectors));
        }
    }
    None
}

fn trigger_source_label(source: &crate::traits::TriggerSource) -> &'static str {
    match source {
        crate::traits::TriggerSource::SlashCommand => "slash_command",
        crate::traits::TriggerSource::DirectMessage => "direct_message",
        crate::traits::TriggerSource::Mention => "mention",
        crate::traits::TriggerSource::ReplyToBot => "reply_to_bot",
        crate::traits::TriggerSource::Ambient => "ambient",
    }
}

fn scope_kind_label(scope: &crate::traits::ConversationScopeKind) -> &'static str {
    match scope {
        crate::traits::ConversationScopeKind::Direct => "direct",
        crate::traits::ConversationScopeKind::Room => "room",
        crate::traits::ConversationScopeKind::Thread => "thread",
        crate::traits::ConversationScopeKind::Topic => "topic",
    }
}

async fn understand_setup_request(
    base_url: &str,
    api_token: &str,
    msg: &ChannelMessage,
    session_id: Option<&str>,
    text: &str,
) -> anyhow::Result<SetupUnderstandResponse> {
    let client = reqwest::Client::new();
    let request = SetupUnderstandRequest {
        surface: "channel",
        session_id,
        text,
        channel: &msg.channel,
        trigger: SetupTriggerPayload {
            source: trigger_source_label(&msg.trigger.source),
            is_direct_message: msg.trigger.is_direct_message,
            was_explicitly_mentioned: msg.trigger.was_explicitly_mentioned,
            is_reply_to_bot: msg.trigger.is_reply_to_bot,
        },
        scope: SetupScopePayload {
            kind: scope_kind_label(&msg.scope.kind),
            id: &msg.scope.id,
        },
    };
    let resp = add_auth(
        client
            .post(format!("{base_url}/setup/understand"))
            .json(&request),
        api_token,
    )
    .send()
    .await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("setup/understand failed ({}): {}", status, body);
    }
    Ok(serde_json::from_str(&body)?)
}

async fn remember_setup_clarifier(
    conversation_key: String,
    clarifier: &SetupClarifier,
    original_text: String,
    setup_clarifiers: &SetupClarifierMap,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let pending = PendingSetupClarifier {
        intent_options: clarifier.options.iter().map(|row| row.id.clone()).collect(),
        original_text,
        expires_at_ms: now + 5 * 60 * 1000,
    };
    let mut guard = setup_clarifiers.lock().await;
    guard.insert(conversation_key, pending);
}

async fn consume_setup_clarifier_reply(
    conversation_key: &str,
    reply: &str,
    setup_clarifiers: &SetupClarifierMap,
) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut guard = setup_clarifiers.lock().await;
    guard.retain(|_, value| value.expires_at_ms > now);
    let pending = guard.get(conversation_key)?.clone();
    let selected = parse_setup_clarifier_selection(reply, &pending.intent_options)?;
    guard.remove(conversation_key);
    Some(format!("{} {}", pending.original_text, selected))
}

fn parse_setup_clarifier_selection(reply: &str, options: &[String]) -> Option<String> {
    let normalized = reply.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    match normalized.as_str() {
        "1" => return options.first().cloned(),
        "2" => return options.get(1).cloned(),
        "3" => return options.get(2).cloned(),
        _ => {}
    }
    options.iter().find_map(|option| {
        let normalized_option = option.to_ascii_lowercase();
        if normalized == normalized_option
            || normalized.contains(&normalized_option.replace('_', " "))
        {
            Some(option.clone())
        } else {
            None
        }
    })
}

fn format_setup_clarifier_message(clarifier: &SetupClarifier) -> String {
    let mut lines = vec![clarifier.question.clone()];
    for (index, option) in clarifier.options.iter().enumerate() {
        lines.push(format!("{}. {}", index + 1, option.label));
    }
    lines.join("\n")
}

async fn preview_setup_automation(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    thread_key: &str,
    goal: &str,
) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = add_auth(
        client
            .post(format!("{base_url}/pack-builder/preview"))
            .json(&serde_json::json!({
                "session_id": session_id,
                "thread_key": thread_key,
                "goal": goal,
                "auto_apply": false
            })),
        api_token,
    )
    .send()
    .await
    .ok()?;
    let status = resp.status();
    let payload: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some("I understood that as an automation setup request, but I couldn't build a preview right now.".to_string());
    }
    Some(format_pack_builder_preview_message(&payload))
}

fn format_pack_builder_preview_message(payload: &serde_json::Value) -> String {
    let status = payload
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("preview_pending");
    let goal = payload
        .get("goal")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Create a useful automation");
    let connectors = payload
        .get("selected_connectors")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let next_actions = payload
        .get("next_actions")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut lines = vec![
        "Automation setup preview".to_string(),
        format!("Goal: {goal}"),
        format!("Status: {status}"),
    ];
    if !connectors.is_empty() {
        lines.push(format!("Connectors: {}", connectors.join(", ")));
    }
    if !next_actions.is_empty() {
        lines.push("Next steps:".to_string());
        for action in next_actions {
            lines.push(format!("- {action}"));
        }
    } else {
        lines.push("Reply `confirm` to apply this preview or `cancel` to discard it.".to_string());
    }
    lines.join("\n")
}

fn format_setup_guidance_message(response: &SetupUnderstandResponse) -> String {
    match response.intent_kind {
        SetupIntentKind::ProviderSetup => {
            let provider = response
                .slots
                .provider_ids
                .first()
                .cloned()
                .unwrap_or_else(|| "a provider".to_string());
            let model = response.slots.model_ids.first().cloned();
            if let Some(model_id) = model {
                format!(
                    "This looks like provider setup. Configure `{provider}` and set the model to `{model_id}` in Settings. Do not paste API keys into channel chat."
                )
            } else {
                format!(
                    "This looks like provider setup. Configure `{provider}` in Settings. Do not paste API keys into channel chat."
                )
            }
        }
        SetupIntentKind::IntegrationSetup => {
            let target = response
                .slots
                .integration_targets
                .first()
                .cloned()
                .unwrap_or_else(|| "that integration".to_string());
            format!(
                "This looks like an MCP or integration setup request. Open the MCP settings for `{target}` and connect or authorize it there."
            )
        }
        SetupIntentKind::ChannelSetupHelp => {
            let target = response
                .slots
                .channel_targets
                .first()
                .cloned()
                .unwrap_or_else(|| "the channel".to_string());
            format!(
                "This looks like channel setup help. Open the channel settings for `{target}` and confirm the bot token, allowed users, and mention-only settings."
            )
        }
        SetupIntentKind::SetupHelp => {
            "I can help with three setup paths here: provider setup, connecting external tools, or creating an automation. Reply with `1`, `2`, or `3`.".to_string()
        }
        SetupIntentKind::AutomationCreate | SetupIntentKind::General => {
            "I couldn't map that setup request cleanly.".to_string()
        }
    }
}

async fn apply_pending_pack_builder(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    thread_key: &str,
    connectors_override: Option<Vec<String>>,
    secret_refs_confirmed: bool,
) -> Option<String> {
    let client = reqwest::Client::new();
    let pending_resp = add_auth(
        client.get(format!("{base_url}/pack-builder/pending")),
        api_token,
    )
    .query(&[("session_id", session_id), ("thread_key", thread_key)])
    .send()
    .await
    .ok()?;
    let pending_status = pending_resp.status();
    let pending_payload: serde_json::Value = pending_resp.json().await.unwrap_or_default();
    if !pending_status.is_success() {
        return Some("No pending pack-builder plan found for this thread.".to_string());
    }
    let plan_id = pending_payload
        .get("pending")
        .and_then(|v| v.get("plan_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if plan_id.is_empty() {
        return Some("No pending pack-builder plan found for this thread.".to_string());
    }

    let apply_resp = add_auth(
        client.post(format!("{base_url}/pack-builder/apply")),
        api_token,
    )
    .json(&serde_json::json!({
        "plan_id": plan_id,
        "session_id": session_id,
        "thread_key": thread_key,
        "selected_connectors": connectors_override.unwrap_or_default(),
        "approvals": {
            "approve_connector_registration": true,
            "approve_pack_install": true,
            "approve_enable_routines": false
        },
        "secret_refs_confirmed": secret_refs_confirmed
    }))
    .send()
    .await
    .ok()?;
    let status = apply_resp.status();
    let payload: serde_json::Value = apply_resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some(format!(
            "Pack Builder apply failed ({status}): {}",
            payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        ));
    }
    Some(format_pack_builder_apply_message(&payload))
}

async fn cancel_pending_pack_builder(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    thread_key: &str,
) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = add_auth(
        client.post(format!("{base_url}/pack-builder/cancel")),
        api_token,
    )
    .json(&serde_json::json!({
        "session_id": session_id,
        "thread_key": thread_key
    }))
    .send()
    .await
    .ok()?;
    let status = resp.status();
    let payload: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some(format!(
            "Pack Builder cancel failed ({status}): {}",
            payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        ));
    }
    Some("Pack Builder plan cancelled for this thread.".to_string())
}

fn format_pack_builder_apply_message(payload: &serde_json::Value) -> String {
    let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if status == "apply_blocked_missing_secrets" {
        let mut lines = vec![
            "Pack Builder Apply Blocked".to_string(),
            "- Missing required secrets.".to_string(),
        ];
        for row in payload
            .get("required_secrets")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            if let Some(secret) = row.as_str() {
                lines.push(format!("  - {}", secret));
            }
        }
        lines.push("- Set secrets, then reply `confirm` again.".to_string());
        return lines.join("\n");
    }
    if status == "apply_blocked_auth" {
        return "Pack Builder Apply Blocked\n- Connector authentication/setup is required.\n- Complete auth and reply `confirm` again.".to_string();
    }
    let pack_name = payload
        .get("pack_installed")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown-pack");
    let pack_version = payload
        .get("pack_installed")
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    format!(
        "Pack Builder Apply Complete\n- Installed: {} {}\n- Routine state: paused by default",
        pack_name, pack_version
    )
}

#[derive(Debug, Default)]
struct AgentRouteDecision {
    agent: Option<String>,
    tool_allowlist: Option<Vec<String>>,
}

fn route_agent_for_channel_message(content: &str) -> AgentRouteDecision {
    if !is_pack_builder_intent(content) {
        return AgentRouteDecision::default();
    }
    AgentRouteDecision {
        agent: Some("pack_builder".to_string()),
        tool_allowlist: Some(vec![
            "pack_builder".to_string(),
            "question".to_string(),
            "websearch".to_string(),
            "webfetch".to_string(),
        ]),
    }
}

fn build_channel_tool_allowlist(
    route_allowlist: Option<&Vec<String>>,
    tool_prefs: &ChannelToolPreferences,
    security_profile: ChannelSecurityProfile,
) -> Option<Vec<String>> {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let mut allowed = PUBLIC_DEMO_ALLOWED_TOOLS
            .iter()
            .map(|tool| tool.to_string())
            .collect::<Vec<_>>();
        if let Some(route_override) = route_allowlist {
            allowed.retain(|tool| route_override.iter().any(|candidate| candidate == tool));
        }
        allowed.retain(|tool| {
            !tool_prefs
                .disabled_tools
                .iter()
                .any(|disabled| disabled == tool)
        });
        if !tool_prefs.enabled_tools.is_empty() {
            allowed.retain(|tool| {
                tool_prefs
                    .enabled_tools
                    .iter()
                    .any(|enabled| enabled == tool)
            });
        }
        return Some(allowed);
    }

    let pack_builder_override = route_allowlist;
    if let Some(pb) = pack_builder_override {
        return Some(pb.clone());
    }

    if tool_prefs.enabled_tools.is_empty()
        && tool_prefs.disabled_tools.is_empty()
        && tool_prefs.enabled_mcp_servers.is_empty()
    {
        return Some(vec!["*".to_string()]);
    }

    let all_builtin = [
        "read",
        "glob",
        "ls",
        "list",
        "grep",
        "codesearch",
        "search",
        "websearch",
        "webfetch",
        "webfetch_html",
        "bash",
        "write",
        "edit",
        "apply_patch",
        "todowrite",
        "memory_search",
        "memory_store",
        "memory_list",
        "mcp_list",
        "browser_status",
        "browser_open",
        "browser_navigate",
        "browser_snapshot",
        "browser_click",
        "browser_type",
        "browser_press",
        "browser_wait",
        "browser_extract",
        "browser_screenshot",
        "browser_close",
        "skill",
        "task",
        "question",
        "pack_builder",
    ];

    let disabled: std::collections::HashSet<&str> = tool_prefs
        .disabled_tools
        .iter()
        .map(|s| s.as_str())
        .collect();

    let explicit_enabled: std::collections::HashSet<&str> = tool_prefs
        .enabled_tools
        .iter()
        .map(|s| s.as_str())
        .collect();

    let has_explicit_enable = !tool_prefs.enabled_tools.is_empty();
    let mut result = Vec::new();

    for tool in all_builtin {
        if disabled.contains(tool) {
            continue;
        }
        if has_explicit_enable && !explicit_enabled.contains(tool) {
            continue;
        }
        result.push(tool.to_string());
    }

    for server in &tool_prefs.enabled_mcp_servers {
        result.push(format!("mcp.{}.*", mcp_namespace_segment(server)));
    }

    if !tool_prefs.enabled_mcp_servers.is_empty() {
        result.push("mcp_list".to_string());
    }

    if result.is_empty() {
        return None;
    }
    Some(result)
}

fn merge_channel_tool_preferences(
    base: ChannelToolPreferences,
    scoped: ChannelToolPreferences,
) -> ChannelToolPreferences {
    ChannelToolPreferences {
        enabled_tools: merge_unique_strings(base.enabled_tools, scoped.enabled_tools),
        disabled_tools: merge_unique_strings(base.disabled_tools, scoped.disabled_tools),
        enabled_mcp_servers: merge_unique_strings(
            base.enabled_mcp_servers,
            scoped.enabled_mcp_servers,
        ),
    }
}

fn merge_unique_strings(mut base: Vec<String>, overlay: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut merged = Vec::new();

    for value in base.drain(..).chain(overlay.into_iter()) {
        let value = value.trim().to_string();
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        merged.push(value);
    }

    merged
}

fn mcp_namespace_segment(raw: &str) -> String {
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
        "server".to_string()
    } else {
        cleaned.to_string()
    }
}

fn is_pack_builder_intent(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let mentions_pack =
        lower.contains("pack") || lower.contains("automation") || lower.contains("workflow");
    let mentions_create = lower.contains("create")
        || lower.contains("build")
        || lower.contains("make")
        || lower.contains("generate")
        || lower.contains("setup");
    let mentions_external = lower.contains("notion")
        || lower.contains("slack")
        || lower.contains("stripe")
        || lower.contains("mcp")
        || lower.contains("connector")
        || lower.contains("headline")
        || lower.contains("news");
    mentions_pack && mentions_create && mentions_external
}

fn is_zip_attachment(msg: &ChannelMessage) -> bool {
    let candidates = [
        msg.attachment_filename.as_deref(),
        msg.attachment_path.as_deref(),
        msg.attachment_url.as_deref(),
        msg.attachment.as_deref(),
    ];
    candidates.into_iter().flatten().any(has_zip_suffix)
}

fn has_zip_suffix(value: &str) -> bool {
    value
        .split(['?', '#'])
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase()
        .ends_with(".zip")
}

fn parse_trusted_pack_sources(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn source_is_trusted_for_auto_install(msg: &ChannelMessage, trusted: &[String]) -> bool {
    if trusted.is_empty() {
        return false;
    }
    let channel = msg.channel.to_ascii_lowercase();
    let reply_target = msg.reply_target.to_ascii_lowercase();
    let sender = msg.sender.to_ascii_lowercase();
    trusted.iter().any(|rule| {
        if rule == "*" {
            return true;
        }
        let rule = rule.to_ascii_lowercase();
        if rule == channel || rule == sender {
            return true;
        }
        let combined_channel_room = format!("{channel}:{reply_target}");
        if rule == combined_channel_room {
            return true;
        }
        let combined_full = format!("{channel}:{reply_target}:{sender}");
        rule == combined_full
    })
}

async fn handle_pack_attachment_if_present(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
) -> Option<String> {
    let Some(path) = msg.attachment_path.as_deref() else {
        return Some(
            "Detected a .zip attachment. Pack detection requires a local attachment path; this upload did not provide one."
                .to_string(),
        );
    };
    let client = reqwest::Client::new();
    let detect_resp = add_auth(client.post(format!("{base_url}/packs/detect")), api_token)
        .json(&serde_json::json!({
            "path": path,
            "attachment_id": msg.id,
            "connector": msg.channel,
            "channel_id": msg.reply_target,
            "sender_id": msg.sender,
        }))
        .send()
        .await;
    let detect_resp = match detect_resp {
        Ok(resp) => resp,
        Err(err) => {
            warn!("pack detect request failed: {}", err);
            return Some(format!("Pack detection failed: {err}"));
        }
    };
    let status = detect_resp.status();
    let payload: serde_json::Value = detect_resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some(format!(
            "Pack detection failed ({status}): {}",
            payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        ));
    }
    let is_pack = payload
        .get("is_pack")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_pack {
        return None;
    }

    let trusted_raw = std::env::var("TANDEM_PACK_AUTO_INSTALL_TRUSTED_SOURCES").unwrap_or_default();
    let trusted = parse_trusted_pack_sources(&trusted_raw);
    let auto_install = source_is_trusted_for_auto_install(msg, &trusted);
    if !auto_install {
        return Some(format!(
            "Tandem Pack detected in attachment `{}`. Auto-install is disabled for this source.\n\nInstall manually from UI or call `/packs/install_from_attachment` with `attachment_id={}` and `path={}`.",
            msg.attachment_filename
                .as_deref()
                .or(msg.attachment.as_deref())
                .unwrap_or("upload.zip"),
            msg.id,
            path
        ));
    }

    let install_resp = add_auth(
        client.post(format!("{base_url}/packs/install_from_attachment")),
        api_token,
    )
    .json(&serde_json::json!({
        "attachment_id": msg.id,
        "path": path,
        "connector": msg.channel,
        "channel_id": msg.reply_target,
        "sender_id": msg.sender,
    }))
    .send()
    .await;
    let install_resp = match install_resp {
        Ok(resp) => resp,
        Err(err) => {
            warn!("pack install_from_attachment request failed: {}", err);
            return Some(format!("Tandem Pack detected but install failed: {err}"));
        }
    };
    let status = install_resp.status();
    let payload: serde_json::Value = install_resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some(format!(
            "Tandem Pack detected but install failed ({status}): {}",
            payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        ));
    }
    let pack_name = payload
        .get("installed")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let pack_version = payload
        .get("installed")
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    Some(format!(
        "Tandem Pack detected and installed: `{pack_name}` `{pack_version}`."
    ))
}

fn synthesize_attachment_prompt(
    channel: &str,
    attachment: &str,
    user_text: &str,
    resource_key: Option<&str>,
    attachment_path: Option<&str>,
    attachment_url: Option<&str>,
    attachment_filename: Option<&str>,
    attachment_mime: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "Channel upload received from `{channel}`: `{attachment}`."
    )];
    if let Some(name) = attachment_filename {
        lines.push(format!("Attachment filename: `{name}`."));
    }
    if let Some(mime) = attachment_mime {
        lines.push(format!("Attachment MIME type: `{mime}`."));
    }
    if let Some(path) = attachment_path {
        lines.push(format!("Stored local attachment path: `{path}`."));
        lines.push(
            "Use the `read` tool on the local path when the file is text-like or parseable."
                .to_string(),
        );
    }
    if let Some(url) = attachment_url {
        lines.push(format!("Attachment source URL: `{url}`."));
    }
    if let Some(key) = resource_key {
        lines.push(format!("Stored upload reference: `{key}`."));
    }
    if !user_text.trim().is_empty() {
        lines.push(format!("User caption/message: {}", user_text.trim()));
    }
    lines.push(
        "Analyze the attachment directly when your model and tools support this MIME type."
            .to_string(),
    );
    lines.push(
        "If this file type is unsupported, explain what format/model capability is required."
            .to_string(),
    );
    lines.join("\n")
}

fn sanitize_resource_segment(raw: &str) -> String {
    let sanitized = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

async fn persist_channel_attachment_reference(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    msg: &ChannelMessage,
    attachment: &str,
) -> Option<String> {
    let client = reqwest::Client::new();
    let resource_key = format!(
        "run/{}/channel_uploads/{}",
        sanitize_resource_segment(session_id),
        sanitize_resource_segment(&msg.id)
    );
    let stored_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let resource_value = serde_json::json!({
        "session_id": session_id,
        "channel": msg.channel,
        "sender": msg.sender,
        "reply_target": msg.reply_target,
        "message_id": msg.id,
        "attachment": attachment,
        "attachment_url": msg.attachment_url,
        "attachment_path": msg.attachment_path,
        "attachment_mime": msg.attachment_mime,
        "attachment_filename": msg.attachment_filename,
        "user_text": msg.content,
        "received_at": msg.timestamp.to_rfc3339(),
        "stored_at_ms": stored_at_ms
    });

    let resource_resp = add_auth(
        client.put(format!("{base_url}/resource/{resource_key}")),
        api_token,
    )
    .json(&serde_json::json!({
        "value": resource_value,
        "updated_by": format!("channels:{}", msg.channel)
    }))
    .send()
    .await;

    match resource_resp {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(
                "failed to persist upload resource '{}' ({}): {}",
                resource_key, status, body
            );
            return None;
        }
        Err(e) => {
            warn!(
                "failed to persist upload resource '{}': {}",
                resource_key, e
            );
            return None;
        }
    }

    let memory_content = format!(
        "Channel upload recorded: channel={}, attachment={}, session={}, sender={}, resource_key={}, file_path={}, file_url={}",
        msg.channel,
        attachment,
        session_id,
        msg.sender,
        resource_key,
        msg.attachment_path.as_deref().unwrap_or("n/a"),
        msg.attachment_url.as_deref().unwrap_or("n/a")
    );
    let memory_resp = add_auth(client.post(format!("{base_url}/memory/put")), api_token)
        .json(&serde_json::json!({
            "run_id": format!("channel-upload-{}", session_id),
            "partition": {
                "org_id": "local",
                "workspace_id": "channels",
                "project_id": session_id,
                "tier": "session"
            },
            "kind": "note",
            "content": memory_content,
            "artifact_refs": [format!("resource:{}", resource_key)],
            "classification": "internal",
            "metadata": {
                "channel": msg.channel,
                "sender": msg.sender,
                "message_id": msg.id
            }
        }))
        .send()
        .await;

    match memory_resp {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(
                "upload resource saved but memory.put failed for '{}' ({}): {}",
                resource_key, status, body
            );
        }
        Err(e) => {
            warn!(
                "upload resource saved but memory.put request failed for '{}': {}",
                resource_key, e
            );
        }
    }

    Some(resource_key)
}

fn extract_image_urls_and_clean_text(input: &str) -> (String, Vec<String>) {
    let (without_markdown_images, markdown_urls) = strip_markdown_image_links(input);
    let mut urls = markdown_urls;
    for token in without_markdown_images.split_whitespace() {
        let candidate = trim_wrapping_punctuation(token);
        if is_image_url(candidate) && !urls.iter().any(|u| u == candidate) {
            urls.push(candidate.to_string());
        }
    }

    let cleaned = without_markdown_images
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    (cleaned, urls)
}

fn strip_markdown_image_links(input: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(input.len());
    let mut urls = Vec::new();
    let mut i = 0usize;

    while i < input.len() {
        let Some(rel) = input[i..].find("![") else {
            out.push_str(&input[i..]);
            break;
        };
        let start = i + rel;
        out.push_str(&input[i..start]);

        let Some(alt_end_rel) = input[start + 2..].find("](") else {
            out.push_str("![");
            i = start + 2;
            continue;
        };
        let alt_end = start + 2 + alt_end_rel;

        let Some(url_end_rel) = input[alt_end + 2..].find(')') else {
            out.push_str("![");
            i = start + 2;
            continue;
        };
        let url_end = alt_end + 2 + url_end_rel;
        let url = input[alt_end + 2..url_end].trim();

        if is_image_url(url) && !urls.iter().any(|u| u == url) {
            urls.push(url.to_string());
        } else {
            out.push_str(&input[start..=url_end]);
        }

        i = url_end + 1;
    }

    (out, urls)
}

fn trim_wrapping_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    })
}

fn is_image_url(url: &str) -> bool {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }
    let base = url.split(['?', '#']).next().unwrap_or(url);
    let lower = base.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn channel_security_profile(
    channel: &str,
    security_profiles: &ChannelSecurityMap,
) -> ChannelSecurityProfile {
    security_profiles
        .get(channel)
        .copied()
        .unwrap_or(ChannelSecurityProfile::Operator)
}

// ---------------------------------------------------------------------------
// Session management helpers
// ---------------------------------------------------------------------------

fn build_channel_session_permissions(
    security_profile: ChannelSecurityProfile,
) -> Vec<serde_json::Value> {
    match security_profile {
        ChannelSecurityProfile::PublicDemo => vec![
            serde_json::json!({ "permission": "memory_search", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_store", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_list", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "websearch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "webfetch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "webfetch_html", "pattern": "*", "action": "allow" }),
        ],
        _ => vec![
            serde_json::json!({ "permission": "ls", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "list", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "glob", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "search", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "grep", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "codesearch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "read", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_search", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_store", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_list", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "websearch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "webfetch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "webfetch_html", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_status", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_open", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_navigate", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_snapshot", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_click", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_type", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_press", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_wait", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_extract", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_screenshot", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_close", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "mcp*", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "bash", "pattern": "*", "action": "allow" }),
        ],
    }
}

