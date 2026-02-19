//! Session dispatcher — routes incoming channel messages to Tandem sessions.
//!
//! Each unique `{channel_name}:{sender_id}` pair maps to one persistent Tandem
//! session. The mapping is durably persisted to `~/.tandem/channel_sessions.json`.
//!
//! Slash commands (`/new`, `/sessions`, `/resume`, `/status`, `/help`) are
//! intercepted before forwarding to the LLM and handled via the session HTTP API.

use std::collections::HashMap;
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

/// `{channel_name}:{sender_id}` → Tandem `session_id`
pub type SessionMap = Arc<Mutex<HashMap<String, String>>>;

/// Parsed slash command from an incoming message.
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

/// Start all configured channel listeners. Returns a `JoinSet` that the caller
/// can `.abort_all()` on shutdown.
pub async fn start_channel_listeners(config: ChannelsConfig) -> JoinSet<()> {
    let session_map: SessionMap = Arc::new(Mutex::new(HashMap::new()));
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

        // Spawn the listener and the consumer concurrently.
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
            backoff_secs = 1; // clean reconnect — reset backoff
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

/// Look up an existing session or create a new one via the `POST /sessions` API.
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

    // Create a new session tagged with the channel source.
    let client = reqwest::Client::new();
    let source_tag = format!("{}:{}", msg.channel, msg.sender);
    let body = serde_json::json!({
        "metadata": { "source": source_tag }
    });

    let resp = client
        .post(format!("{base_url}/sessions"))
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
        .or_else(|| json.get("sessionID"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;

    session_map
        .lock()
        .await
        .insert(map_key.to_string(), session_id.clone());

    Some(session_id)
}

/// Submit a message to an existing Tandem session and collect the response.
async fn run_in_session(
    session_id: &str,
    content: &str,
    base_url: &str,
    api_token: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    // Append the user message and start a run
    let run_resp = client
        .post(format!("{base_url}/sessions/{session_id}/run"))
        .bearer_auth(api_token)
        .json(&serde_json::json!({ "message": content }))
        .send()
        .await?;

    let run_json: serde_json::Value = run_resp.json().await?;
    let run_id = run_json
        .get("runID")
        .or_else(|| run_json.get("run_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("no run_id in response"))?;

    // Poll the run until finished (simple polling; SSE streaming is a Phase 3 improvement)
    let mut last_text = String::new();
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let status_resp = client
            .get(format!("{base_url}/sessions/{session_id}/run/{run_id}"))
            .bearer_auth(api_token)
            .send()
            .await?;
        let status: serde_json::Value = status_resp.json().await?;
        if let Some(text) = status.get("assistantText").and_then(|v| v.as_str()) {
            last_text = text.to_string();
        }
        let done = status
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| matches!(s, "completed" | "failed" | "cancelled" | "timeout"))
            .unwrap_or(false);
        if done {
            break;
        }
    }

    Ok(last_text)
}

// ---------------------------------------------------------------------------
// Slash command handlers
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
        SlashCommand::Status => {
            status_text(msg, base_url, api_token, session_map).await
        }
        SlashCommand::Rename { name } => {
            rename_session_text(name, msg, base_url, api_token, session_map).await
        }
    }
}

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
    let source_tag = format!("{channel}:{sender}");
    let resp = client
        .get(format!("{base_url}/sessions"))
        .bearer_auth(api_token)
        .send()
        .await;

    let Ok(resp) = resp else {
        return "⚠️ Could not reach Tandem server.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let sessions = json.as_array().cloned().unwrap_or_default();
    let matching: Vec<_> = sessions
        .iter()
        .filter(|s| {
            s.get("metadata")
                .and_then(|m| m.get("source"))
                .and_then(|v| v.as_str())
                .map(|src| src == source_tag)
                .unwrap_or(false)
        })
        .take(5)
        .enumerate()
        .map(|(i, s)| {
            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let name = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");
            format!("{}. {} \"{}\"", i + 1, &id[..8.min(id.len())], name)
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
    let source_tag = map_key.clone();
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({ "metadata": { "source": source_tag } });
    if let Some(ref n) = name {
        body["title"] = serde_json::Value::String(n.clone());
    }
    let resp = client
        .post(format!("{base_url}/sessions"))
        .bearer_auth(api_token)
        .json(&body)
        .send()
        .await;
    let Ok(resp) = resp else {
        return "⚠️ Could not create session.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };
    let session_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    session_map
        .lock()
        .await
        .insert(map_key, session_id.clone());
    let display_name = name.unwrap_or_else(|| "Untitled".to_string());
    format!(
        "✅ Started new session \"{display_name}\" ({})\nFresh context — what would you like to work on?",
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
    let source = map_key.clone();
    let client = reqwest::Client::new();
    let Ok(resp) = client
        .get(format!("{base_url}/sessions"))
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
        let src_match = s
            .get("metadata")
            .and_then(|m| m.get("source"))
            .and_then(|v| v.as_str())
            .map(|src| src == source)
            .unwrap_or(false);
        if !src_match {
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
            session_map.lock().await.insert(map_key, id.to_string());
            format!(
                "✅ Resumed session \"{title}\" ({})\n→ Ready to continue.",
                &id[..8.min(id.len())]
            )
        }
        None => format!("⚠️ No session matching \"{query}\" found. Use /sessions to list yours."),
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
        .get(format!("{base_url}/sessions/{sid}"))
        .bearer_auth(api_token)
        .send()
        .await
    else {
        return format!("ℹ️ Session: {}", &sid[..8.min(sid.len())]);
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return format!("ℹ️ Session: {}", &sid[..8.min(sid.len())]);
    };
    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");
    let msgs = json
        .get("messageCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    format!(
        "ℹ️ Session: \"{title}\" ({}) | {} messages",
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
    let _ = client
        .patch(format!("{base_url}/sessions/{sid}"))
        .bearer_auth(api_token)
        .json(&serde_json::json!({ "title": name }))
        .send()
        .await;
    format!("✅ Session renamed to \"{name}\".")
}
