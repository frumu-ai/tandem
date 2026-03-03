//! Session dispatcher — routes incoming channel messages to Tandem sessions.
//!
//! Each unique `{channel_name}:{sender_id}` pair maps to one persistent Tandem
//! session. The mapping is durably persisted under Tandem's app-data state dir
//! (for example `~/.local/share/tandem/data/channel_sessions.json` on Linux)
//! and reloaded on startup.
//!
//! ## API paths (tandem-server)
//!
//! | Action         | Path                                 |
//! |----------------|--------------------------------------|
//! | Create session | `POST /session`                      |
//! | List sessions  | `GET  /session`                      |
//! | Get session    | `GET  /session/{id}`                 |
//! | Update session | `PUT  /session/{id}`                 |
//! | Prompt (sync)  | `POST /session/{id}/prompt_sync`     |
//!
//! ## Slash commands
//!
//! `/new [name]`, `/sessions`, `/resume <query>`, `/rename <name>`,
//! `/status`, `/run`, `/cancel`, `/todos`, `/requests`, `/answer <id> <text>`,
//! `/providers`, `/models [provider]`, `/model <model_id>`, `/approve <tool_call_id>`,
//! `/deny <tool_call_id>`, `/help`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::config::ChannelsConfig;
use crate::discord::DiscordChannel;
use crate::slack::SlackChannel;
use crate::telegram::TelegramChannel;
use crate::traits::{Channel, ChannelMessage, SendMessage};

// ---------------------------------------------------------------------------
// Auth helper
// ---------------------------------------------------------------------------

/// Attach both auth schemes so the dispatcher works regardless of whether the
/// Tandem server is running in headless mode (Bearer) or via the Tauri sidecar
/// (x-tandem-token).
fn add_auth(rb: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    rb.header("x-tandem-token", token).bearer_auth(token)
}

// ---------------------------------------------------------------------------
// Session map + persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub created_at_ms: u64,
    pub last_seen_at_ms: u64,
    pub channel: String,
    pub sender: String,
}

/// `{channel_name}:{sender_id}` → Tandem `SessionRecord`
pub type SessionMap = Arc<Mutex<HashMap<String, SessionRecord>>>;

fn persistence_path() -> PathBuf {
    let base = std::env::var("TANDEM_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if let Some(data_dir) = dirs::data_dir() {
                return data_dir.join("tandem").join("data");
            }
            dirs::home_dir()
                .map(|home| home.join(".tandem").join("data"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        });
    base.join("channel_sessions.json")
}

/// Load the session map from disk. Returns an empty map if the file doesn't
/// exist or cannot be parsed.
async fn load_session_map() -> HashMap<String, SessionRecord> {
    let path = persistence_path();
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return HashMap::new();
    };

    if let Ok(map) = serde_json::from_slice::<HashMap<String, SessionRecord>>(&bytes) {
        return map;
    }

    // Migration from old String format
    if let Ok(old_map) = serde_json::from_slice::<HashMap<String, String>>(&bytes) {
        let mut new_map = HashMap::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        for (key, session_id) in old_map {
            let mut parts = key.splitn(2, ':');
            let channel = parts.next().unwrap_or("unknown").to_string();
            let sender = parts.next().unwrap_or("unknown").to_string();
            new_map.insert(
                key,
                SessionRecord {
                    session_id,
                    created_at_ms: now,
                    last_seen_at_ms: now,
                    channel,
                    sender,
                },
            );
        }
        return new_map;
    }

    HashMap::new()
}

/// Persist the session map to disk. Silently ignores I/O errors.
async fn save_session_map(map: &HashMap<String, SessionRecord>) {
    let path = persistence_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(json) = serde_json::to_vec_pretty(map) {
        let _ = tokio::fs::write(&path, json).await;
    }
}

// ---------------------------------------------------------------------------
// Slash command parsing
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum SlashCommand {
    New { name: Option<String> },
    ListSessions,
    Resume { query: String },
    Rename { name: String },
    Status,
    Run,
    Cancel,
    Todos,
    Requests,
    Answer { question_id: String, answer: String },
    Providers,
    Models { provider: Option<String> },
    Model { model_id: String },
    Help,
    Approve { tool_call_id: String },
    Deny { tool_call_id: String },
}

fn parse_slash_command(content: &str) -> Option<SlashCommand> {
    let trimmed = content.trim();
    if trimmed == "/new" {
        return Some(SlashCommand::New { name: None });
    }
    if let Some(name) = trimmed.strip_prefix("/new ") {
        return Some(SlashCommand::New {
            name: Some(name.trim().to_string()),
        });
    }
    if trimmed == "/sessions" || trimmed == "/session" {
        return Some(SlashCommand::ListSessions);
    }
    if let Some(q) = trimmed.strip_prefix("/resume ") {
        return Some(SlashCommand::Resume {
            query: q.trim().to_string(),
        });
    }
    if let Some(name) = trimmed.strip_prefix("/rename ") {
        return Some(SlashCommand::Rename {
            name: name.trim().to_string(),
        });
    }
    if trimmed == "/status" {
        return Some(SlashCommand::Status);
    }
    if trimmed == "/run" {
        return Some(SlashCommand::Run);
    }
    if trimmed == "/cancel" || trimmed == "/abort" {
        return Some(SlashCommand::Cancel);
    }
    if trimmed == "/todos" || trimmed == "/todo" {
        return Some(SlashCommand::Todos);
    }
    if trimmed == "/requests" {
        return Some(SlashCommand::Requests);
    }
    if trimmed == "/providers" {
        return Some(SlashCommand::Providers);
    }
    if trimmed == "/models" {
        return Some(SlashCommand::Models { provider: None });
    }
    if let Some(provider) = trimmed.strip_prefix("/models ") {
        return Some(SlashCommand::Models {
            provider: Some(provider.trim().to_string()),
        });
    }
    if let Some(model_id) = trimmed.strip_prefix("/model ") {
        let model_id = model_id.trim();
        if !model_id.is_empty() {
            return Some(SlashCommand::Model {
                model_id: model_id.to_string(),
            });
        }
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("/answer ") {
        let mut parts = rest.trim().splitn(2, ' ');
        let question_id = parts.next().unwrap_or_default().trim();
        let answer = parts.next().unwrap_or_default().trim();
        if !question_id.is_empty() && !answer.is_empty() {
            return Some(SlashCommand::Answer {
                question_id: question_id.to_string(),
                answer: answer.to_string(),
            });
        }
        return None;
    }
    if trimmed == "/help" || trimmed == "/?" {
        return Some(SlashCommand::Help);
    }
    if let Some(id) = trimmed.strip_prefix("/approve ") {
        return Some(SlashCommand::Approve {
            tool_call_id: id.trim().to_string(),
        });
    }
    if let Some(id) = trimmed.strip_prefix("/deny ") {
        return Some(SlashCommand::Deny {
            tool_call_id: id.trim().to_string(),
        });
    }
    None
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Start all configured channel listeners. Returns a `JoinSet` that the caller
/// can `.abort_all()` on shutdown.
pub async fn start_channel_listeners(config: ChannelsConfig) -> JoinSet<()> {
    let initial_map = load_session_map().await;
    info!(
        "tandem-channels: loaded {} persisted session mappings",
        initial_map.len()
    );

    let session_map: SessionMap = Arc::new(Mutex::new(initial_map));
    let mut set = JoinSet::new();

    if let Some(tg) = config.telegram {
        let channel = Arc::new(TelegramChannel::new(tg));
        let map = session_map.clone();
        let base_url = config.server_base_url.clone();
        let api_token = config.api_token.clone();
        set.spawn(supervise(channel, base_url, api_token, map));
        info!("tandem-channels: Telegram listener started");
    }

    if let Some(dc) = config.discord {
        let channel = Arc::new(DiscordChannel::new(dc));
        let map = session_map.clone();
        let base_url = config.server_base_url.clone();
        let api_token = config.api_token.clone();
        set.spawn(supervise(channel, base_url, api_token, map));
        info!("tandem-channels: Discord listener started");
    }

    if let Some(sl) = config.slack {
        let channel = Arc::new(SlackChannel::new(sl));
        let map = session_map.clone();
        let base_url = config.server_base_url.clone();
        let api_token = config.api_token.clone();
        set.spawn(supervise(channel, base_url, api_token, map));
        info!("tandem-channels: Slack listener started");
    }

    set
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

/// Runs a channel listener with exponential-backoff restart on failure.
async fn supervise(
    channel: Arc<dyn Channel>,
    base_url: String,
    api_token: String,
    session_map: SessionMap,
) {
    let mut backoff_secs: u64 = 1;
    loop {
        let (tx, mut rx) = mpsc::channel::<ChannelMessage>(64);

        let channel_listen = channel.clone();
        let listen_handle = tokio::spawn(async move {
            if let Err(e) = channel_listen.listen(tx).await {
                error!("channel listener error: {e}");
            }
        });

        while let Some(msg) = rx.recv().await {
            let ch = channel.clone();
            let base = base_url.clone();
            let tok = api_token.clone();
            let map = session_map.clone();
            tokio::spawn(async move {
                process_channel_message(msg, ch, &base, &tok, &map).await;
            });
        }

        listen_handle.abort();

        if channel.health_check().await {
            backoff_secs = 1;
        } else {
            warn!(
                "channel '{}' unhealthy — restarting in {}s",
                channel.name(),
                backoff_secs
            );
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(60);
        }
    }
}

// ---------------------------------------------------------------------------
// Message processing
// ---------------------------------------------------------------------------

/// Process a single incoming channel message: handle slash commands or forward
/// to the Tandem session HTTP API.
async fn process_channel_message(
    msg: ChannelMessage,
    channel: Arc<dyn Channel>,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) {
    // --- Slash command intercept ---
    if msg.content.starts_with('/') {
        if let Some(cmd) = parse_slash_command(&msg.content) {
            let response = handle_slash_command(cmd, &msg, base_url, api_token, session_map).await;
            if let Err(e) = channel
                .send(&SendMessage {
                    content: response,
                    recipient: msg.reply_target.clone(),
                    image_urls: Vec::new(),
                })
                .await
            {
                error!(
                    "failed to send slash-command response via channel '{}': {e}",
                    channel.name()
                );
            }
            return;
        }
    }

    // --- Normal message → Tandem session ---
    let map_key = format!("{}:{}", msg.channel, msg.sender);
    let session_id = get_or_create_session(&map_key, &msg, base_url, api_token, session_map).await;

    let session_id = match session_id {
        Some(id) => id,
        None => {
            error!("failed to get or create session for {}", map_key);
            return;
        }
    };

    if let Err(e) = channel.start_typing(&msg.reply_target).await {
        warn!(
            "failed to start typing indicator for channel '{}': {e}",
            channel.name()
        );
    }
    let mut prompt_content = msg.content.clone();
    if let Some(attachment) = msg.attachment.as_deref() {
        if is_zip_attachment(&msg) {
            if let Some(pack_reply) =
                handle_pack_attachment_if_present(&msg, base_url, api_token).await
            {
                if let Err(e) = channel.stop_typing(&msg.reply_target).await {
                    warn!(
                        "failed to stop typing indicator for channel '{}': {e}",
                        channel.name()
                    );
                }
                if let Err(e) = channel
                    .send(&SendMessage {
                        content: pack_reply,
                        recipient: msg.reply_target.clone(),
                        image_urls: Vec::new(),
                    })
                    .await
                {
                    error!(
                        "failed to send pack ingestion reply via '{}': {e}",
                        channel.name()
                    );
                }
                return;
            }
        }
        let persisted = persist_channel_attachment_reference(
            base_url,
            api_token,
            &session_id,
            &msg,
            attachment,
        )
        .await;
        prompt_content = synthesize_attachment_prompt(
            &msg.channel,
            attachment,
            &msg.content,
            persisted.as_deref(),
            msg.attachment_path.as_deref(),
            msg.attachment_url.as_deref(),
            msg.attachment_filename.as_deref(),
            msg.attachment_mime.as_deref(),
        );
    }

    let route = route_agent_for_channel_message(&msg.content);

    let response = run_in_session(
        &session_id,
        &prompt_content,
        base_url,
        api_token,
        msg.attachment_path.as_deref(),
        msg.attachment_url.as_deref(),
        msg.attachment_mime.as_deref(),
        msg.attachment_filename.as_deref(),
        route.agent.as_deref(),
        route.tool_allowlist.as_ref(),
    )
    .await;
    if let Err(e) = channel.stop_typing(&msg.reply_target).await {
        warn!(
            "failed to stop typing indicator for channel '{}': {e}",
            channel.name()
        );
    }

    let reply = response.unwrap_or_else(|e| format!("⚠️ Error: {e}"));
    let (reply_text, image_urls) = extract_image_urls_and_clean_text(&reply);
    if let Err(e) = channel
        .send(&SendMessage {
            content: reply_text,
            recipient: msg.reply_target,
            image_urls,
        })
        .await
    {
        error!("failed to send channel reply via '{}': {e}", channel.name());
    }
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

// ---------------------------------------------------------------------------
// Session management helpers
// ---------------------------------------------------------------------------

fn build_channel_session_create_body(title: &str) -> serde_json::Value {
    serde_json::json!({
        "title": title,
        "directory": ".",
        "permission": [
            { "permission": "ls", "pattern": "*", "action": "allow" },
            { "permission": "list", "pattern": "*", "action": "allow" },
            { "permission": "glob", "pattern": "*", "action": "allow" },
            { "permission": "search", "pattern": "*", "action": "allow" },
            { "permission": "grep", "pattern": "*", "action": "allow" },
            { "permission": "codesearch", "pattern": "*", "action": "allow" },
            { "permission": "read", "pattern": "*", "action": "allow" },
            { "permission": "websearch", "pattern": "*", "action": "allow" },
            { "permission": "webfetch", "pattern": "*", "action": "allow" },
            { "permission": "webfetch_html", "pattern": "*", "action": "allow" },
            { "permission": "bash", "pattern": "*", "action": "allow" }
        ]
    })
}

/// Look up an existing session or create a new one via `POST /session`.
async fn get_or_create_session(
    map_key: &str,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> Option<String> {
    {
        let mut guard = session_map.lock().await;
        if let Some(record) = guard.get_mut(map_key) {
            record.last_seen_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let sid = record.session_id.clone();
            // Persist the updated last_seen_at_ms
            save_session_map(&guard).await;
            return Some(sid);
        }
    }

    let client = reqwest::Client::new();
    let title = format!("{} — {}", msg.channel, msg.sender);
    let body = build_channel_session_create_body(&title);

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
        map_key.to_string(),
        SessionRecord {
            session_id: session_id.clone(),
            created_at_ms: now,
            last_seen_at_ms: now,
            channel: msg.channel.clone(),
            sender: msg.sender.clone(),
        },
    );
    save_session_map(&guard).await;

    Some(session_id)
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
    if let Ok(Some(model)) = fetch_default_model_spec(&client, base_url, api_token).await {
        body["model"] = model;
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

    let sse_resp = add_auth(client.get(&event_url), api_token)
        .header("Accept", "text/event-stream")
        .send()
        .await?;

    use futures_util::StreamExt;
    let mut content_buf = String::new();
    let mut last_error: Option<String> = None;
    let mut body_stream = sse_resp.bytes_stream();
    let mut line_buf = String::new();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    'outer: loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        match tokio::time::timeout(Duration::from_secs(60), body_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                line_buf.push_str(&String::from_utf8_lossy(&chunk));
            }
            Ok(Some(Err(e))) => {
                tracing::warn!("SSE stream error: {e}");
                break 'outer;
            }
            Ok(None) | Err(_) => break 'outer,
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

// ---------------------------------------------------------------------------
// Slash command handler dispatch
// ---------------------------------------------------------------------------

async fn handle_slash_command(
    cmd: SlashCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    match cmd {
        SlashCommand::Help => help_text(),
        SlashCommand::ListSessions => {
            list_sessions_text(base_url, api_token, &msg.channel, &msg.sender).await
        }
        SlashCommand::New { name } => {
            new_session_text(name, msg, base_url, api_token, session_map).await
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
            let map_key = format!("{}:{}", msg.channel, msg.sender);
            let session_id = {
                let guard = session_map.lock().await;
                guard.get(&map_key).map(|r| r.session_id.clone())
            };
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
            let map_key = format!("{}:{}", msg.channel, msg.sender);
            let session_id = {
                let guard = session_map.lock().await;
                guard.get(&map_key).map(|r| r.session_id.clone())
            };
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
    }
}

// ---------------------------------------------------------------------------
// Individual slash command implementations
// ---------------------------------------------------------------------------

fn help_text() -> String {
    "🤖 *Tandem Commands*\n\
    /new [name] — start a fresh session\n\
    /sessions — list your recent sessions\n\
    /resume <id or name> — switch to a previous session\n\
    /rename <name> — rename the current session\n\
    /status — show current session info\n\
    /run — show active run state\n\
    /cancel — cancel the active run\n\
    /todos — list current session todos\n\
    /requests — list pending tool/question requests\n\
    /answer <question_id> <text> — answer a pending question\n\
    /providers — list available providers\n\
    /models [provider] — list models by provider\n\
    /model <model_id> — set model for current default provider\n\
    /approve <tool_call_id> — approve a pending tool call\n\
    /deny <tool_call_id> — deny a pending tool call\n\
    /help — show this message"
        .to_string()
}

async fn active_session_id(msg: &ChannelMessage, session_map: &SessionMap) -> Option<String> {
    let map_key = format!("{}:{}", msg.channel, msg.sender);
    session_map
        .lock()
        .await
        .get(&map_key)
        .map(|r| r.session_id.clone())
}

async fn list_sessions_text(
    base_url: &str,
    api_token: &str,
    channel: &str,
    sender: &str,
) -> String {
    let client = reqwest::Client::new();
    let source_title_prefix = format!("{channel} — {sender}");

    let Ok(resp) = add_auth(client.get(format!("{base_url}/session")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not reach Tandem server.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let sessions = json.as_array().cloned().unwrap_or_default();
    // Filter to sessions whose title starts with "{channel} — {sender}"
    let matching: Vec<_> = sessions
        .iter()
        .filter(|s| {
            s.get("title")
                .and_then(|t| t.as_str())
                .map(|t| t.starts_with(&source_title_prefix))
                .unwrap_or(false)
        })
        .take(5)
        .enumerate()
        .map(|(i, s)| {
            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");
            let msg_count = s
                .get("messages")
                .and_then(|m| m.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            format!(
                "{}. `{}` — {} ({} msgs)",
                i + 1,
                &id[..8.min(id.len())],
                title,
                msg_count
            )
        })
        .collect();

    if matching.is_empty() {
        "📋 No previous sessions found.".to_string()
    } else {
        format!("📋 Your sessions:\n{}", matching.join("\n"))
    }
}

async fn new_session_text(
    name: Option<String>,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let map_key = format!("{}:{}", msg.channel, msg.sender);
    let display_name = name
        .clone()
        .unwrap_or_else(|| format!("{} — {}", msg.channel, msg.sender));
    let client = reqwest::Client::new();
    let body = build_channel_session_create_body(&display_name);

    let Ok(resp) = add_auth(client.post(format!("{base_url}/session")), api_token)
        .json(&body)
        .send()
        .await
    else {
        return "⚠️ Could not create session.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let session_id = match json.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return "⚠️ Server returned no session ID.".to_string(),
    };

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
        },
    );
    save_session_map(&guard).await;

    format!(
        "✅ Started new session \"{}\" (`{}`)\nFresh context — what would you like to work on?",
        display_name,
        &session_id[..8.min(session_id.len())]
    )
}

async fn resume_session_text(
    query: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let map_key = format!("{}:{}", msg.channel, msg.sender);
    let source_prefix = format!("{} — {}", msg.channel, msg.sender);
    let client = reqwest::Client::new();

    let Ok(resp) = add_auth(client.get(format!("{base_url}/session")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not reach server.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let sessions = json.as_array().cloned().unwrap_or_default();
    let found = sessions.iter().find(|s| {
        // Only search sessions belonging to this sender
        let title_ok = s
            .get("title")
            .and_then(|t| t.as_str())
            .map(|t| t.starts_with(&source_prefix))
            .unwrap_or(false);
        if !title_ok {
            return false;
        }
        let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let title = s
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        id.starts_with(&query) || title.contains(&query.to_lowercase())
    });

    match found {
        Some(s) => {
            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");

            let mut guard = session_map.lock().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            guard.insert(
                map_key,
                SessionRecord {
                    session_id: id.to_string(),
                    created_at_ms: now,
                    last_seen_at_ms: now,
                    channel: msg.channel.clone(),
                    sender: msg.sender.clone(),
                },
            );
            save_session_map(&guard).await;

            format!(
                "✅ Resumed session \"{}\" (`{}`)\n→ Ready to continue.",
                title,
                &id[..8.min(id.len())]
            )
        }
        None => format!(
            "⚠️ No session matching \"{}\" found. Use /sessions to list yours.",
            query
        ),
    }
}

async fn status_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let map_key = format!("{}:{}", msg.channel, msg.sender);
    let session_id = session_map
        .lock()
        .await
        .get(&map_key)
        .map(|r| r.session_id.clone());
    let Some(sid) = session_id else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };

    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/session/{sid}")), api_token)
        .send()
        .await
    else {
        return format!("ℹ️ Session: `{}`", &sid[..8.min(sid.len())]);
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return format!("ℹ️ Session: `{}`", &sid[..8.min(sid.len())]);
    };

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");
    let msgs = json
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    format!(
        "ℹ️ Session: \"{}\" (`{}`) | {} messages",
        title,
        &sid[..8.min(sid.len())],
        msgs
    )
}

async fn rename_session_text(
    name: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let map_key = format!("{}:{}", msg.channel, msg.sender);
    let session_id = session_map
        .lock()
        .await
        .get(&map_key)
        .map(|r| r.session_id.clone());
    let Some(sid) = session_id else {
        return "⚠️ No active session to rename. Send a message first.".to_string();
    };

    let client = reqwest::Client::new();
    let resp = add_auth(client.patch(format!("{base_url}/session/{sid}")), api_token)
        .json(&serde_json::json!({ "title": name }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => format!("✅ Session renamed to \"{name}\"."),
        Ok(r) => format!("⚠️ Rename failed (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Rename failed: {e}"),
    }
}

async fn run_status_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };

    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.get(format!("{base_url}/session/{sid}/run")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not fetch run status.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected run status response.".to_string();
    };
    let active = json
        .get("active")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if active.is_null() {
        return "ℹ️ No active run.".to_string();
    }

    let run_id = active
        .get("run_id")
        .or_else(|| active.get("runID"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    format!(
        "🏃 Active run: `{}` on session `{}`",
        &run_id[..8.min(run_id.len())],
        &sid[..8.min(sid.len())]
    )
}

async fn cancel_run_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "⚠️ No active session — nothing to cancel.".to_string();
    };
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.post(format!("{base_url}/session/{sid}/cancel")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not reach server to cancel.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Cancel requested, but response could not be parsed.".to_string();
    };
    let cancelled = json
        .get("cancelled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if cancelled {
        "🛑 Cancelled active run.".to_string()
    } else {
        "ℹ️ No active run to cancel.".to_string()
    }
}

async fn todos_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.get(format!("{base_url}/session/{sid}/todo")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not fetch todos.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected todos response.".to_string();
    };

    let Some(items) = json.as_array() else {
        return "⚠️ Todos response was not a list.".to_string();
    };
    if items.is_empty() {
        return "✅ No todos in this session.".to_string();
    }

    let lines = items
        .iter()
        .take(12)
        .enumerate()
        .map(|(i, item)| {
            let content = item
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");
            let icon = if status.eq_ignore_ascii_case("completed") {
                "✅"
            } else if status.eq_ignore_ascii_case("in_progress") {
                "⏳"
            } else {
                "⬜"
            };
            format!("{}. {} {} ({})", i + 1, icon, content, status)
        })
        .collect::<Vec<_>>();
    format!("🧾 Session todos:\n{}", lines.join("\n"))
}

fn value_str<'a>(obj: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| obj.get(*key).and_then(|v| v.as_str()))
}

fn value_bool(obj: &serde_json::Value, key: &str) -> Option<bool> {
    obj.get(key).and_then(|v| v.as_bool())
}

fn session_matches(value: &serde_json::Value, session_id: &str) -> bool {
    value_str(value, &["session_id", "sessionID", "sessionId"])
        .map(|v| v == session_id)
        .unwrap_or(false)
}

async fn requests_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let sid = active_session_id(msg, session_map).await;
    let client = reqwest::Client::new();

    let permissions = match add_auth(client.get(format!("{base_url}/permission")), api_token)
        .send()
        .await
    {
        Ok(resp) => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("requests").cloned())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let questions = match add_auth(client.get(format!("{base_url}/question")), api_token)
        .send()
        .await
    {
        Ok(resp) => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let filtered_permissions: Vec<_> = if let Some(session_id) = sid.as_ref() {
        permissions
            .into_iter()
            .filter(|v| session_matches(v, session_id))
            .collect()
    } else {
        permissions
    };
    let filtered_questions: Vec<_> = if let Some(session_id) = sid.as_ref() {
        questions
            .into_iter()
            .filter(|v| session_matches(v, session_id))
            .collect()
    } else {
        questions
    };

    if filtered_permissions.is_empty() && filtered_questions.is_empty() {
        return "✅ No pending requests.".to_string();
    }

    let mut lines = Vec::new();
    for req in filtered_permissions.iter().take(8) {
        let id = value_str(req, &["id", "requestID", "request_id"]).unwrap_or("?");
        let tool = value_str(req, &["tool", "tool_name", "name"]).unwrap_or("tool");
        let approved = value_bool(req, "approved");
        let status = if approved == Some(true) {
            "approved"
        } else {
            "pending"
        };
        lines.push(format!(
            "🔐 `{}` {} ({})",
            &id[..8.min(id.len())],
            tool,
            status
        ));
    }
    for q in filtered_questions.iter().take(8) {
        let id = value_str(q, &["id", "questionID", "question_id"]).unwrap_or("?");
        let prompt = value_str(q, &["prompt", "question", "text"]).unwrap_or("question");
        lines.push(format!(
            "❓ `{}` {}",
            &id[..8.min(id.len())],
            prompt.chars().take(80).collect::<String>()
        ));
    }

    format!(
        "🧷 Pending requests ({} tool, {} question):\n{}",
        filtered_permissions.len(),
        filtered_questions.len(),
        lines.join("\n")
    )
}

async fn answer_question_text(
    question_id: String,
    answer: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "⚠️ No active session — cannot answer question.".to_string();
    };
    let client = reqwest::Client::new();
    let url = format!("{base_url}/sessions/{sid}/questions/{question_id}/answer");
    let resp = add_auth(client.post(url), api_token)
        .json(&serde_json::json!({ "answer": answer }))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            format!("✅ Answer submitted for question `{question_id}`.")
        }
        Ok(r) => format!("⚠️ Could not answer question (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Could not answer question: {e}"),
    }
}

async fn providers_text(base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch providers.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected providers response.".to_string();
    };
    let default = json
        .get("default")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let all = json
        .get("all")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if all.is_empty() {
        return "ℹ️ No providers available.".to_string();
    }
    let lines = all
        .iter()
        .take(20)
        .map(|entry| {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let model_count = entry
                .get("models")
                .and_then(|v| v.as_object())
                .map(|m| m.len())
                .unwrap_or(0);
            format!("• {} ({} models)", id, model_count)
        })
        .collect::<Vec<_>>();
    format!("🧠 Providers (default: `{default}`):\n{}", lines.join("\n"))
}

async fn models_text(provider: Option<String>, base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch models.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected models response.".to_string();
    };
    let all = json
        .get("all")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if all.is_empty() {
        return "ℹ️ No providers/models available.".to_string();
    }

    if let Some(provider_id) = provider {
        let target = all.iter().find(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.eq_ignore_ascii_case(&provider_id))
                .unwrap_or(false)
        });
        let Some(entry) = target else {
            return format!("⚠️ Provider `{provider_id}` not found. Use /providers.");
        };
        let models = entry
            .get("models")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        if models.is_empty() {
            return format!("ℹ️ Provider `{provider_id}` has no models listed.");
        }
        let mut model_ids = models.keys().cloned().collect::<Vec<_>>();
        model_ids.sort();
        let lines = model_ids
            .iter()
            .take(30)
            .map(|m| format!("• {m}"))
            .collect::<Vec<_>>();
        return format!("🧠 Models for `{provider_id}`:\n{}", lines.join("\n"));
    }

    let lines = all
        .iter()
        .map(|entry| {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let count = entry
                .get("models")
                .and_then(|v| v.as_object())
                .map(|m| m.len())
                .unwrap_or(0);
            format!("• {}: {} models", id, count)
        })
        .collect::<Vec<_>>();
    format!(
        "🧠 Model catalog by provider:\n{}\nUse `/models <provider>` to list model IDs.",
        lines.join("\n")
    )
}

async fn set_model_text(model_id: String, base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch provider catalog.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected provider catalog response.".to_string();
    };

    let Some(default_provider) = json.get("default").and_then(|v| v.as_str()) else {
        return "⚠️ No default provider configured. Use desktop/TUI provider setup first."
            .to_string();
    };

    let provider_entry = json.get("all").and_then(|v| v.as_array()).and_then(|all| {
        all.iter().find(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id == default_provider)
                .unwrap_or(false)
        })
    });

    if let Some(entry) = provider_entry {
        let known = entry
            .get("models")
            .and_then(|v| v.as_object())
            .map(|models| models.contains_key(&model_id))
            .unwrap_or(true);
        if !known {
            return format!(
                "⚠️ Model `{}` not found for provider `{}`. Use `/models {}` first.",
                model_id, default_provider, default_provider
            );
        }
    }

    let mut provider_patch = serde_json::Map::new();
    provider_patch.insert(
        "default_model".to_string(),
        serde_json::json!(model_id.clone()),
    );
    let mut providers_patch = serde_json::Map::new();
    providers_patch.insert(
        default_provider.to_string(),
        serde_json::Value::Object(provider_patch),
    );
    let mut patch_map = serde_json::Map::new();
    patch_map.insert(
        "providers".to_string(),
        serde_json::Value::Object(providers_patch),
    );
    let patch = serde_json::Value::Object(patch_map);

    let resp = add_auth(client.patch(format!("{base_url}/config")), api_token)
        .json(&patch)
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            format!(
                "✅ Model set to `{}` for default provider `{}`.",
                model_id, default_provider
            )
        }
        Ok(r) => format!("⚠️ Could not set model (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Could not set model: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Slash command parser ──────────────────────────────────────────────

    #[test]
    fn parse_new_no_name() {
        assert!(matches!(
            parse_slash_command("/new"),
            Some(SlashCommand::New { name: None })
        ));
    }

    #[test]
    fn parse_new_with_name() {
        let cmd = parse_slash_command("/new my session");
        assert!(matches!(
            cmd,
            Some(SlashCommand::New { name: Some(ref n) }) if n == "my session"
        ));
    }

    #[test]
    fn parse_sessions() {
        assert!(matches!(
            parse_slash_command("/sessions"),
            Some(SlashCommand::ListSessions)
        ));
        assert!(matches!(
            parse_slash_command("/session"),
            Some(SlashCommand::ListSessions)
        ));
    }

    #[test]
    fn parse_resume() {
        let cmd = parse_slash_command("/resume abc123");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Resume { ref query }) if query == "abc123"
        ));
    }

    #[test]
    fn parse_rename() {
        let cmd = parse_slash_command("/rename new name");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Rename { ref name }) if name == "new name"
        ));
    }

    #[test]
    fn parse_status() {
        assert!(matches!(
            parse_slash_command("/status"),
            Some(SlashCommand::Status)
        ));
    }

    #[test]
    fn parse_run() {
        assert!(matches!(
            parse_slash_command("/run"),
            Some(SlashCommand::Run)
        ));
    }

    #[test]
    fn parse_cancel_aliases() {
        assert!(matches!(
            parse_slash_command("/cancel"),
            Some(SlashCommand::Cancel)
        ));
        assert!(matches!(
            parse_slash_command("/abort"),
            Some(SlashCommand::Cancel)
        ));
    }

    #[test]
    fn parse_todos_aliases() {
        assert!(matches!(
            parse_slash_command("/todos"),
            Some(SlashCommand::Todos)
        ));
        assert!(matches!(
            parse_slash_command("/todo"),
            Some(SlashCommand::Todos)
        ));
    }

    #[test]
    fn parse_requests() {
        assert!(matches!(
            parse_slash_command("/requests"),
            Some(SlashCommand::Requests)
        ));
    }

    #[test]
    fn parse_answer() {
        let cmd = parse_slash_command("/answer q123 continue with option A");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Answer { ref question_id, ref answer })
            if question_id == "q123" && answer == "continue with option A"
        ));
    }

    #[test]
    fn parse_providers() {
        assert!(matches!(
            parse_slash_command("/providers"),
            Some(SlashCommand::Providers)
        ));
    }

    #[test]
    fn parse_models() {
        assert!(matches!(
            parse_slash_command("/models"),
            Some(SlashCommand::Models { provider: None })
        ));
        let cmd = parse_slash_command("/models openrouter");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Models { provider: Some(ref p) }) if p == "openrouter"
        ));
    }

    #[test]
    fn parse_model_set() {
        let cmd = parse_slash_command("/model gpt-5-mini");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Model { ref model_id }) if model_id == "gpt-5-mini"
        ));
    }

    #[test]
    fn parse_help() {
        assert!(matches!(
            parse_slash_command("/help"),
            Some(SlashCommand::Help)
        ));
        assert!(matches!(
            parse_slash_command("/?"),
            Some(SlashCommand::Help)
        ));
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert!(parse_slash_command("/unknown").is_none());
        assert!(parse_slash_command("not a command").is_none());
        assert!(parse_slash_command("").is_none());
    }

    #[test]
    fn parse_trims_whitespace() {
        assert!(matches!(
            parse_slash_command("  /help  "),
            Some(SlashCommand::Help)
        ));
    }

    #[test]
    fn detects_pack_builder_intent() {
        let text = "create me a pack that checks latest headline news and posts to slack";
        assert!(is_pack_builder_intent(text));
        let route = route_agent_for_channel_message(text);
        assert_eq!(route.agent.as_deref(), Some("pack_builder"));
        assert!(route
            .tool_allowlist
            .as_ref()
            .is_some_and(|v| v.iter().any(|t| t == "pack_builder")));
    }

    #[test]
    fn non_pack_intent_uses_default_route() {
        let text = "what model am I using?";
        assert!(!is_pack_builder_intent(text));
        let route = route_agent_for_channel_message(text);
        assert!(route.agent.is_none());
        assert!(route.tool_allowlist.is_none());
    }

    // ── SessionRecord ─────────────────────────────────────────────────────

    #[test]
    fn session_record_roundtrip() {
        let record = SessionRecord {
            session_id: "s1".to_string(),
            created_at_ms: 1000,
            last_seen_at_ms: 2000,
            channel: "telegram".to_string(),
            sender: "user1".to_string(),
        };
        let serialized = serde_json::to_string(&record).unwrap();
        let deserialized: SessionRecord = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.session_id, "s1");
        assert_eq!(deserialized.created_at_ms, 1000);
        assert_eq!(deserialized.last_seen_at_ms, 2000);
        assert_eq!(deserialized.channel, "telegram");
        assert_eq!(deserialized.sender, "user1");
    }

    #[test]
    fn extracts_markdown_image_and_cleans_text() {
        let input = "Here is the render:\n![chart](https://cdn.example.com/chart.png)\nLooks good.";
        let (text, urls) = extract_image_urls_and_clean_text(input);
        assert_eq!(urls, vec!["https://cdn.example.com/chart.png"]);
        assert!(text.contains("Here is the render:"));
        assert!(text.contains("Looks good."));
        assert!(!text.contains("![chart]"));
    }

    #[test]
    fn extracts_direct_image_url_token() {
        let input = "Generated image: https://example.com/out/final.webp";
        let (text, urls) = extract_image_urls_and_clean_text(input);
        assert_eq!(urls, vec!["https://example.com/out/final.webp"]);
        assert!(text.contains("Generated image:"));
    }

    #[test]
    fn synthesize_attachment_prompt_includes_reference_when_present() {
        let out = synthesize_attachment_prompt(
            "telegram",
            "photo",
            "please analyze",
            Some("run/s1/channel_uploads/u1"),
            Some("/tmp/photo.jpg"),
            Some("https://example.com/photo.jpg"),
            Some("photo.jpg"),
            Some("image/jpeg"),
        );
        assert!(out.contains("Channel upload received"));
        assert!(out.contains("Stored upload reference"));
        assert!(out.contains("Stored local attachment path"));
        assert!(out.contains("please analyze"));
    }

    #[test]
    fn sanitize_resource_segment_replaces_invalid_chars() {
        assert_eq!(
            sanitize_resource_segment("abc/def:ghi"),
            "abc_def_ghi".to_string()
        );
    }

    #[test]
    fn zip_attachment_detection_handles_filename_path_and_url() {
        let mut msg = ChannelMessage {
            id: "m1".to_string(),
            sender: "u1".to_string(),
            reply_target: "c1".to_string(),
            content: "hello".to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: Some("pack.zip".to_string()),
        };
        assert!(is_zip_attachment(&msg));
        msg.attachment_filename = None;
        msg.attachment_path = Some("/tmp/upload.PACK.ZIP".to_string());
        assert!(is_zip_attachment(&msg));
        msg.attachment_path = None;
        msg.attachment_url = Some("https://example.com/x/y/pack.zip?sig=1".to_string());
        assert!(is_zip_attachment(&msg));
    }

    #[test]
    fn trusted_source_matching_supports_channel_room_sender_scopes() {
        let msg = ChannelMessage {
            id: "m1".to_string(),
            sender: "userA".to_string(),
            reply_target: "room1".to_string(),
            content: "hello".to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: None,
        };
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord".to_string()]
        ));
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord:room1".to_string()]
        ));
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord:room1:usera".to_string()]
        ));
        assert!(!source_is_trusted_for_auto_install(
            &msg,
            &["slack".to_string()]
        ));
    }
}
