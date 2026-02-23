//! Session dispatcher — routes incoming channel messages to Tandem sessions.
//!
//! Each unique `{channel_name}:{sender_id}` pair maps to one persistent Tandem
//! session. The mapping is durably persisted to `~/.local/share/tandem/channel_sessions.json`
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
    // Prefer XDG_DATA_HOME, fall back to ~/.local/share
    let base = std::env::var("TANDEM_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_path()
                .unwrap_or_else(|| PathBuf::from(".tandem"))
                .join("tandem")
        });
    base.join("channel_sessions.json")
}

fn dirs_path() -> Option<PathBuf> {
    // Unix: ~/.local/share / Windows: %APPDATA%
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|home| PathBuf::from(home).join(".local").join("share"))
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
            let _ = channel
                .send(&SendMessage {
                    content: response,
                    recipient: msg.reply_target.clone(),
                })
                .await;
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

    let _ = channel.start_typing(&msg.reply_target).await;
    let response = run_in_session(&session_id, &msg.content, base_url, api_token).await;
    let _ = channel.stop_typing(&msg.reply_target).await;

    let reply = response.unwrap_or_else(|e| format!("⚠️ Error: {e}"));
    let _ = channel
        .send(&SendMessage {
            content: reply,
            recipient: msg.reply_target,
        })
        .await;
}

// ---------------------------------------------------------------------------
// Session management helpers
// ---------------------------------------------------------------------------

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
    let body = serde_json::json!({
        "title": format!("{} — {}", msg.channel, msg.sender),
        "directory": "."
    });

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
) -> anyhow::Result<String> {
    let timeout_secs: u64 = std::env::var("TANDEM_CHANNEL_MAX_WAIT_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs + 30))
        .build()?;

    let body = serde_json::json!({
        "parts": [{ "type": "text", "text": content }]
    });

    // Request run metadata so we can bind SSE to this specific run.
    let resp = add_auth(
        client.post(format!(
            "{base_url}/session/{session_id}/prompt_async?return=run"
        )),
        api_token,
    )
    .json(&body)
    .send()
    .await?;

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
    let run_id = fire_json
        .get("runID")
        .or_else(|| fire_json.get("run_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Stream the SSE event bus until the run finishes or we timeout.
    let event_url = format!(
        "{base_url}/event?sessionID={session_id}{}",
        if run_id.is_empty() {
            String::new()
        } else {
            format!("&runID={run_id}")
        }
    );

    let sse_resp = add_auth(client.get(&event_url), api_token)
        .header("Accept", "text/event-stream")
        .send()
        .await?;

    use futures_util::StreamExt;
    let mut content_buf = String::new();
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
                "session.run.finished" | "done" => break 'outer,
                _ => {}
            }
        }
    }

    Ok(if content_buf.is_empty() {
        "(no response)".to_string()
    } else {
        content_buf
    })
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
    let body = serde_json::json!({ "title": display_name, "directory": "." });

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
}
