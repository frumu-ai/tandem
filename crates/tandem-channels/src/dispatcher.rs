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
//! `/status`, `/help`

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
// Session map + persistence
// ---------------------------------------------------------------------------

/// `{channel_name}:{sender_id}` → Tandem `session_id`
pub type SessionMap = Arc<Mutex<HashMap<String, String>>>;

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
async fn load_session_map() -> HashMap<String, String> {
    let path = persistence_path();
    match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Persist the session map to disk. Silently ignores I/O errors.
async fn save_session_map(map: &HashMap<String, String>) {
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
    Help,
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
    if trimmed == "/help" || trimmed == "/?" {
        return Some(SlashCommand::Help);
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
        let guard = session_map.lock().await;
        if let Some(id) = guard.get(map_key) {
            return Some(id.clone());
        }
    }

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "title": format!("{} — {}", msg.channel, msg.sender),
        "directory": "."
    });

    let resp = client
        .post(format!("{base_url}/session"))
        .bearer_auth(api_token)
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
    guard.insert(map_key.to_string(), session_id.clone());
    save_session_map(&*guard).await;

    Some(session_id)
}

/// Submit a message to an existing Tandem session and collect the last
/// assistant text via `POST /session/{id}/prompt_sync`.
///
/// The request body is `SendMessageRequest { parts: [{ type: "text", text: "..." }] }`.
/// The response is a `Vec<WireSessionMessage>`. We extract the last assistant
/// message's text parts and join them.
async fn run_in_session(
    session_id: &str,
    content: &str,
    base_url: &str,
    api_token: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        // Give the LLM up to 5 minutes for long responses
        .timeout(Duration::from_secs(300))
        .build()?;

    let body = serde_json::json!({
        "parts": [{ "type": "text", "text": content }]
    });

    let resp = client
        .post(format!("{base_url}/session/{session_id}/prompt_sync"))
        .bearer_auth(api_token)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("prompt_sync failed ({status}): {err}");
    }

    let messages: serde_json::Value = resp.json().await?;

    // Find the last assistant message and concatenate its text parts.
    let assistant_text = extract_last_assistant_text(&messages);
    Ok(assistant_text)
}

/// Extract the last assistant message's text from the `WireSessionMessage[]`
/// returned by `prompt_sync`.
///
/// Each message is `{ info: { role: "..." }, parts: [{ type: "text", text: "..." }] }`.
fn extract_last_assistant_text(messages: &serde_json::Value) -> String {
    let arr = match messages.as_array() {
        Some(a) => a,
        None => return String::new(),
    };

    let last_assistant = arr.iter().rev().find(|m| {
        m.get("info")
            .and_then(|i| i.get("role"))
            .and_then(|r| r.as_str())
            .map(|r| r == "assistant")
            .unwrap_or(false)
    });

    let msg = match last_assistant {
        Some(m) => m,
        None => return String::new(),
    };

    let parts = msg.get("parts").and_then(|p| p.as_array());
    let Some(parts) = parts else {
        return String::new();
    };

    parts
        .iter()
        .filter(|p| {
            p.get("type")
                .and_then(|t| t.as_str())
                .map(|t| t == "text")
                .unwrap_or(false)
        })
        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
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
        SlashCommand::Rename { name } => {
            rename_session_text(name, msg, base_url, api_token, session_map).await
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
    /help — show this message"
        .to_string()
}

async fn list_sessions_text(
    base_url: &str,
    api_token: &str,
    channel: &str,
    sender: &str,
) -> String {
    let client = reqwest::Client::new();
    let source_title_prefix = format!("{channel} — {sender}");

    let Ok(resp) = client
        .get(format!("{base_url}/session"))
        .bearer_auth(api_token)
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

    let Ok(resp) = client
        .post(format!("{base_url}/session"))
        .bearer_auth(api_token)
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
    guard.insert(map_key, session_id.clone());
    save_session_map(&*guard).await;

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

    let Ok(resp) = client
        .get(format!("{base_url}/session"))
        .bearer_auth(api_token)
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
            guard.insert(map_key, id.to_string());
            save_session_map(&*guard).await;

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
    let session_id = session_map.lock().await.get(&map_key).cloned();
    let Some(sid) = session_id else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };

    let client = reqwest::Client::new();
    let Ok(resp) = client
        .get(format!("{base_url}/session/{sid}"))
        .bearer_auth(api_token)
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
    let session_id = session_map.lock().await.get(&map_key).cloned();
    let Some(sid) = session_id else {
        return "⚠️ No active session to rename. Send a message first.".to_string();
    };

    let client = reqwest::Client::new();
    let resp = client
        .patch(format!("{base_url}/session/{sid}"))
        .bearer_auth(api_token)
        .json(&serde_json::json!({ "title": name }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => format!("✅ Session renamed to \"{name}\"."),
        Ok(r) => format!("⚠️ Rename failed (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Rename failed: {e}"),
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

    // ── Assistant text extraction ─────────────────────────────────────────

    #[test]
    fn extract_assistant_text_from_prompt_sync_response() {
        let messages = serde_json::json!([
            {
                "info": { "role": "user", "sessionID": "s1", "id": "m1",
                           "time": { "created": 0 } },
                "parts": [{ "type": "text", "text": "hello" }]
            },
            {
                "info": { "role": "assistant", "sessionID": "s1", "id": "m2",
                           "time": { "created": 1 } },
                "parts": [
                    { "type": "text", "text": "Hi there!" },
                    { "type": "text", "text": "How can I help?" }
                ]
            }
        ]);
        let text = extract_last_assistant_text(&messages);
        assert_eq!(text, "Hi there!\nHow can I help?");
    }

    #[test]
    fn extract_skips_tool_invocation_parts() {
        let messages = serde_json::json!([
            {
                "info": { "role": "assistant", "sessionID": "s1", "id": "m1",
                           "time": { "created": 0 } },
                "parts": [
                    { "type": "tool_invocation", "tool": "bash", "args": {} },
                    { "type": "text", "text": "Done!" }
                ]
            }
        ]);
        let text = extract_last_assistant_text(&messages);
        assert_eq!(text, "Done!");
    }

    #[test]
    fn extract_empty_when_no_assistant_message() {
        let messages = serde_json::json!([
            {
                "info": { "role": "user", "sessionID": "s1", "id": "m1",
                           "time": { "created": 0 } },
                "parts": [{ "type": "text", "text": "hello" }]
            }
        ]);
        assert_eq!(extract_last_assistant_text(&messages), "");
    }

    #[test]
    fn extract_empty_on_empty_array() {
        let messages = serde_json::json!([]);
        assert_eq!(extract_last_assistant_text(&messages), "");
    }

    #[test]
    fn extract_uses_last_assistant_message() {
        let messages = serde_json::json!([
            {
                "info": { "role": "assistant", "sessionID": "s1", "id": "m1",
                           "time": { "created": 0 } },
                "parts": [{ "type": "text", "text": "first" }]
            },
            {
                "info": { "role": "assistant", "sessionID": "s1", "id": "m2",
                           "time": { "created": 1 } },
                "parts": [{ "type": "text", "text": "second" }]
            }
        ]);
        assert_eq!(extract_last_assistant_text(&messages), "second");
    }
}
